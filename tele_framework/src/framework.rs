use async_trait::async_trait;
use std::sync::Arc;
use paste::paste;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

type WSResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait LogicalModule: Send + Sync {
    type View: Send + Sync;
    
    fn name(&self) -> &str;
    async fn init(view: Self::View) -> WSResult<Self> where Self: Sized;
    async fn shutdown(&self) -> WSResult<()>;
}

pub struct Framework {
    modules: Vec<u8>,
}

#[macro_export]
macro_rules! define_module {
    ($module:ident $(, ($field:ident, $dep_type:ty))*) => {
        paste! {
            // 定义该模块的View trait - 只包含需要访问的模块函数
            pub trait [<$module ViewTrait>]: Send + Sync {
                $(
                    fn $field(&self) -> &$dep_type;
                )*
                fn initialized(&self) -> &::std::sync::Mutex<bool>;
                fn shutdown(&self) -> &::std::sync::Mutex<bool>;
            }

            // 定义View结构体
            pub struct [<$module View>] {
                framework: ::std::sync::Arc<dyn [<$module ViewTrait>]>
            }

            // View实现
            impl [<$module View>] {
                pub fn new(framework: ::std::sync::Arc<dyn [<$module ViewTrait>]>) -> Self {
                    Self { framework }
                }

                $(
                    pub fn $field(&self) -> &$dep_type {
                        self.framework.$field()
                    }
                )*

                pub fn initialized(&self) -> &::std::sync::Mutex<bool> {
                    self.framework.initialized()
                }

                pub fn shutdown(&self) -> &::std::sync::Mutex<bool> {
                    self.framework.shutdown()
                }
            }
        }
    };
}

#[macro_export]
macro_rules! define_framework {
    ($first:ident: $first_type:ty $(, $rest:ident: $rest_type:ty)*) => {
        paste! {
            // 为每个字段定义大小常量
            const [<$first:upper _SIZE>]: usize = ::std::mem::size_of::<$first_type>();
            $(
                const [<$rest:upper _SIZE>]: usize = ::std::mem::size_of::<$rest_type>();
            )*

            // 为每个字段定义偏移常量
            const [<$first:upper _OFFSET>]: usize = 0;
            $(
                const [<$rest:upper _OFFSET>]: usize = [<$first:upper _OFFSET>] + [<$first:upper _SIZE>];
            )*

            // Framework实现每个模块的ViewTrait
            impl [<$first_type ViewTrait>] for Framework {
                fn initialized(&self) -> &::std::sync::Mutex<bool> {
                    unsafe {
                        let ptr = self.modules.as_ptr() as *const $first_type;
                        &(*ptr).initialized
                    }
                }
                fn shutdown(&self) -> &::std::sync::Mutex<bool> {
                    unsafe {
                        let ptr = self.modules.as_ptr() as *const $first_type;
                        &(*ptr).shutdown
                    }
                }
            }

            $(
                impl [<$rest_type ViewTrait>] for Framework {
                    fn $first(&self) -> &$first_type {
                        unsafe {
                            let ptr = self.modules.as_ptr() as *const $first_type;
                            &*ptr
                        }
                    }
                    fn a(&self) -> &$rest_type {
                        unsafe {
                            let ptr = (self.modules.as_ptr().add([<$rest:upper _OFFSET>])) as *const $rest_type;
                            &*ptr
                        }
                    }
                    fn initialized(&self) -> &::std::sync::Mutex<bool> {
                        unsafe {
                            let ptr = (self.modules.as_ptr().add([<$rest:upper _OFFSET>])) as *const $rest_type;
                            &(*ptr).initialized
                        }
                    }
                    fn shutdown(&self) -> &::std::sync::Mutex<bool> {
                        unsafe {
                            let ptr = (self.modules.as_ptr().add([<$rest:upper _OFFSET>])) as *const $rest_type;
                            &(*ptr).shutdown
                        }
                    }
                }
            )*

            impl Framework {
                // 获取各个模块的 view
                pub fn [<$first _view>](&self) -> [<$first_type View>] {
                    let arc: ::std::sync::Arc<dyn [<$first_type ViewTrait>]> = ::std::sync::Arc::new(Framework {
                        modules: self.modules.clone()
                    });
                    [<$first_type View>]::new(arc)
                }

                $(
                    pub fn [<$rest _view>](&self) -> [<$rest_type View>] {
                        let arc: ::std::sync::Arc<dyn [<$rest_type ViewTrait>]> = ::std::sync::Arc::new(Framework {
                            modules: self.modules.clone()
                        });
                        [<$rest_type View>]::new(arc)
                    }
                )*

                pub async fn init(&mut self) -> $crate::framework::WSResult<()> {
                    let total_size = [<$first:upper _SIZE>] $(+ [<$rest:upper _SIZE>])*;
                    self.modules = Vec::with_capacity(total_size);
                    
                    // 初始化第一个模块
                    let first_module = <$first_type>::init(self.[<$first _view>]()).await?;
                    let ptr = &first_module as *const $first_type;
                    unsafe {
                        self.modules.extend_from_slice(::std::slice::from_raw_parts(
                            ptr as *const u8, 
                            [<$first:upper _SIZE>]
                        ));
                    }
                    std::mem::forget(first_module);

                    // 初始化其他模块
                    $(
                        let module = <$rest_type>::init(self.[<$rest _view>]()).await?;
                        let ptr = &module as *const $rest_type;
                        unsafe {
                            self.modules.extend_from_slice(::std::slice::from_raw_parts(
                                ptr as *const u8, 
                                [<$rest:upper _SIZE>]
                            ));
                        }
                        std::mem::forget(module);
                    )*

                    Ok(())
                }

                pub async fn shutdown(&self) -> $crate::framework::WSResult<()> {
                    // 关闭第一个模块
                    let first_module = unsafe {
                        &*(self.modules.as_ptr() as *const $first_type)
                    };
                    first_module.shutdown().await?;

                    // 关闭其他模块
                    $(
                        let module = unsafe {
                            &*(self.modules.as_ptr().add([<$rest:upper _OFFSET>]) as *const $rest_type)
                        };
                        module.shutdown().await?;
                    )*

                    Ok(())
                }
            }
        }
    };
}

impl Framework {
    pub fn new() -> Self {
        Self {
            modules: Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // 先定义 TestModuleB
    pub struct TestModuleB {
        _phantom: std::marker::PhantomData<()>,
        pub initialized: Mutex<bool>,
        pub shutdown: Mutex<bool>
    }

    #[async_trait]
    impl LogicalModule for TestModuleB {
        type View = TestModuleBView;

        fn name(&self) -> &str {
            "TestModuleB"
        }

        async fn init(_view: Self::View) -> WSResult<Self> {
            Ok(Self {
                _phantom: std::marker::PhantomData,
                initialized: Mutex::new(true),
                shutdown: Mutex::new(false)
            })
        }

        async fn shutdown(&self) -> WSResult<()> {
            *self.shutdown.lock().unwrap() = true;
            Ok(())
        }
    }

    define_module!(TestModuleB);

    // 然后定义 TestModuleA
    pub struct TestModuleA {
        _phantom: std::marker::PhantomData<()>,
        pub initialized: Mutex<bool>,
        pub shutdown: Mutex<bool>
    }

    #[async_trait]
    impl LogicalModule for TestModuleA {
        type View = TestModuleAView;

        fn name(&self) -> &str {
            "TestModuleA"
        }

        async fn init(_view: Self::View) -> WSResult<Self> {
            Ok(Self {
                _phantom: std::marker::PhantomData,
                initialized: Mutex::new(true),
                shutdown: Mutex::new(false)
            })
        }

        async fn shutdown(&self) -> WSResult<()> {
            *self.shutdown.lock().unwrap() = true;
            Ok(())
        }
    }

    define_module!(TestModuleA, 
        (a, TestModuleA),
        (b, TestModuleB)
    );

    define_framework! {
        b: TestModuleB,
        a: TestModuleA
    }

    #[test]
    fn test_module_size() {
        println!("Size of TestModuleA: {}", std::mem::size_of::<TestModuleA>());
        println!("Size of TestModuleB: {}", std::mem::size_of::<TestModuleB>());
        println!("Size of TestModuleAView: {}", std::mem::size_of::<TestModuleAView>());
        println!("Size of TestModuleBView: {}", std::mem::size_of::<TestModuleBView>());
    }

    #[tokio::test]
    async fn test_framework() {
        println!("Starting test_framework");
        
        let mut fw = Framework::new();
        println!("Created new framework");
        
        fw.init().await.unwrap();
        println!("Initialized framework");
        
        // 通过 framework 获取 TestModuleA 的 view
        let view = fw.a_view();
        
        // 验证 TestModuleA 已初始化
        assert!(*view.a().initialized.lock().unwrap());
        assert!(!*view.a().shutdown.lock().unwrap());
        
        // 验证 TestModuleB 已初始化
        assert!(*view.b().initialized.lock().unwrap());
        assert!(!*view.b().shutdown.lock().unwrap());
        
        println!("TestModuleB name: {}", view.b().name());
        assert_eq!(view.b().name(), "TestModuleB");
        
        fw.shutdown().await.unwrap();
        println!("Shutdown framework");
        
        // 验证模块已关闭
        let view = fw.a_view();
        assert!(*view.a().shutdown.lock().unwrap());
        assert!(*view.b().shutdown.lock().unwrap());
    }
}

