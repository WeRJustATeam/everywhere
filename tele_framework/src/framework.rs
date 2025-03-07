use async_trait::async_trait;
use std::sync::Arc;
use paste::paste;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub type WSResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;


#[async_trait]
pub trait LogicalModule: Send + Sync {
    type View: Send + Sync;
    type NewArg: Send + Sync;
    
    fn name(&self) -> &str;
    async fn init(view: Self::View, arg: Self::NewArg) -> WSResult<Self> where Self: Sized;
    async fn shutdown(&self) -> WSResult<()>;
}


#[macro_export]
macro_rules! define_module {
    ($module:ident $(, ($field:ident, $dep_type:ty))*) => {
        paste! {
            
            // 定义模块的AccessTrait
            #[async_trait::async_trait]
            pub trait [<$module AccessTrait>]: Send + Sync {
                fn [<$module:snake>](&self) -> &$module;
            }

            // 创建一个新trait，将所有依赖的AccessTrait作为supertrait
            pub trait [<$module ViewTrait>]: Send + Sync $(+ [<$dep_type AccessTrait>])* {
                // 这个trait可以为空，它的目的只是组合多个AccessTrait
            }
            
            // 为所有实现了必要AccessTrait的类型自动实现ViewTrait
            impl<T> [<$module ViewTrait>] for T 
            where 
                T: Send + Sync $(+ [<$dep_type AccessTrait>])* 
            {}

            // 定义View结构体
            pub struct [<$module View>] {
                pub view: std::sync::Weak<dyn [<$module ViewTrait>]>
            }

            // View实现
            impl [<$module View>] {
                pub fn new(view: &Arc<dyn [<$module ViewTrait>]>) -> Self {
                    //println!("new view of {}", stringify!($module));
                    Self { 
                        view: Arc::downgrade(view)
                    }
                }

                // 获取每个依赖模块
                $(
                    pub fn [<$dep_type:snake>](&self) -> &$dep_type {
                        //println!("accessing {}", stringify!($dep_type));
                        let ret=unsafe {
                            // 直接访问，不需要安全检查，因为这是在框架内部使用
                            let ptr = std::ptr::NonNull::new(self.view.as_ptr() as *const _ as *mut _).unwrap();
                            let view_ref: &dyn [<$module ViewTrait>] = ptr.as_ref();
                            let module_ptr=std::ptr::NonNull::new(view_ref.[<$dep_type:snake>]() as *const _ as *mut _).unwrap();
                            module_ptr.as_ref()
                        };
                        //println!("accessed {}", stringify!($dep_type));
                        ret
                    }
                )*
            }
        }
    };
}

#[macro_export]
macro_rules! define_framework {
    ($first:ident: $first_type:ty $(, $rest:ident: $rest_type:ty)*) => {
        paste! {
            // expand the modules
            #[derive(Default)]
            pub struct FrameworkInner {
                [<$first_type:snake>]: Option<$first_type>,
                $(
                    [<$rest_type:snake>]: Option<$rest_type>,
                )*
            }

            pub struct Framework(Arc<FrameworkInner>);

            impl Framework {
                pub fn new() -> Self {
                    Self(Arc::new(FrameworkInner::default()))
                }
            }

            // 为Framework实现各个模块的AccessTrait
            #[async_trait::async_trait]
            impl [<$first_type AccessTrait>] for FrameworkInner {
                fn [<$first_type:snake>](&self) -> &$first_type {
                    self.[<$first_type:snake>].as_ref().unwrap()
                }
            }
            
            $(
                #[async_trait::async_trait]
                impl [<$rest_type AccessTrait>] for FrameworkInner {
                    fn [<$rest_type:snake>](&self) -> &$rest_type {
                        self.[<$rest_type:snake>].as_ref().unwrap()
                    }
                }
            )*
            
            // Framework已经实现了所有AccessTrait，它自动实现了各ViewTrait
            
            // 定义框架参数结构体
            pub struct FrameworkArgs {
                pub [<$first _arg>]: [<$first_type NewArg>],
                $(
                    pub [<$rest _arg>]: [<$rest_type NewArg>],
                )*
            }

            
            // // 实现ViewTrait需要的方法（需要安全地访问其他模块）
            // impl [<$first_type ViewTrait>] for Framework {
            //     fn $first(&self) -> &$first_type {
            //         unsafe {
            //             let ptr = self.modules.as_ptr() as *const $first_type;
            //             &*ptr
            //         }
            //     }
            // }
            
            // $(
            //     impl [<$rest_type ViewTrait>] for Framework {
            //         fn $rest(&self) -> &$rest_type {
            //             unsafe {
            //                 let ptr = (self.modules.as_ptr().add(::std::mem::size_of::<$first_type>())) as *const $rest_type;
            //                 &*ptr
            //             }
            //         }
            //     }
            // )*
            
            // 实现FrameworkTrait
            impl Framework {
                async fn init(&self, args: FrameworkArgs) -> WSResult<()> {
                    let total_size = ::std::mem::size_of::<$first_type>() $(+ ::std::mem::size_of::<$rest_type>())*;
                    // 使用内部可变性修改modules
                    // let mut fw = Arc::get_mut(&mut self.0).expect("Arc should be unique");
                    // fw.modules = Vec::with_capacity(total_size);
                    let fw: &mut FrameworkInner = unsafe { ::std::ptr::NonNull::new(
                        // deref the arc and get ref of inner
                        ((&*self.0) as *const _ as *mut _)
                    ).unwrap().as_mut()};
                    
                    // 初始化第一个模块
                    
                    let [<inited_ $first_type:snake>]: Option<$first_type> = Some(<$first_type>::init(self.[<$first _view>](), args.[<$first _arg>]).await?);
                    //println!("initializing {}", stringify!($first_type));
                    fw.[<$first_type:snake>] = [<inited_ $first_type:snake>];
                    //println!("initialized {}", stringify!($first_type));
                    
                    // 初始化其他模块
                    $( 
                        //println!("bf initializing {}", stringify!($rest_type));
                        let [<inited_ $rest_type:snake>]: Option<$rest_type> = Some(<$rest_type>::init(self.[<$rest _view>](), args.[<$rest _arg>]).await?);
                        //println!("initializing {}", stringify!($rest_type));
                        fw.[<$rest_type:snake>] = [<inited_ $rest_type:snake>];
                        //println!("initialized {}", stringify!($rest_type));
                    )*

                    //println!("initialized");
                    tracing::info!("framework initialized");
                    Ok(())
                }
                
                async fn shutdown(&self) -> WSResult<()> {
                    
                    self.0.[<$first_type:snake>].as_ref().unwrap().shutdown().await?;
                    $(
                        self.0.[<$rest_type:snake>].as_ref().unwrap().shutdown().await?;
                    )*
                    
                    Ok(())
                }
                
                fn [<$first _view>](&self) -> [<$first_type View>] {
                    //println!("getting view of {}", stringify!($first_type));
                    // 先克隆self得到Framework实例，再装箱为Arc
                    let framework = self.0.clone();
                    //println!("got arc of framework");
                    let framework_arc: Arc<dyn [<$first_type ViewTrait>]> = framework;
                    //println!("got dyn view trait of {}", stringify!($first_type));
                    [<$first_type View>]::new(&framework_arc)
                }
                
                $(
                    fn [<$rest _view>](&self) -> [<$rest_type View>] {
                        //println!("getting view of {}", stringify!($rest_type));
                        // 先克隆self得到Framework实例，再装箱为Arc
                        let framework = self.0.clone();
                        //println!("got arc of framework");
                        let framework_arc: Arc<dyn [<$rest_type ViewTrait>]> = framework;
                        //println!("got dyn view trait of {}", stringify!($rest_type));
                        [<$rest_type View>]::new(&framework_arc)
                    }
                )*
            }
        }
    };
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
        type NewArg = TestModuleBNewArg;

        fn name(&self) -> &str {
            "TestModuleB"
        }

        async fn init(_view: Self::View, _arg: Self::NewArg) -> WSResult<Self> {
            //println!("initializing TestModuleB");
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
        type NewArg = TestModuleANewArg;

        fn name(&self) -> &str {
            "TestModuleA"
        }

        async fn init(_view: Self::View, _arg: Self::NewArg) -> WSResult<Self> {
            //println!("initializing TestModuleA");
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

    // 定义测试模块的NewArg
    pub struct TestModuleANewArg;
    pub struct TestModuleBNewArg;

    
    define_framework! {
        a: TestModuleA,
        b: TestModuleB
    }

    #[test]
    fn test_module_size() {
        //println!("Size of TestModuleA: {}", std::mem::size_of::<TestModuleA>());
        //println!("Size of TestModuleB: {}", std::mem::size_of::<TestModuleB>());
        //println!("Size of TestModuleAView: {}", std::mem::size_of::<TestModuleAView>());
        //println!("Size of TestModuleBView: {}", std::mem::size_of::<TestModuleBView>());
    }

    #[tokio::test]
    async fn test_framework() {
        // init tracing
        let _=tracing_subscriber::fmt::try_init();

        //println!("Starting test_framework");
        
        let mut fw = Framework::new();
        //println!("Created new framework");
        
        // 创建测试参数
        let args = FrameworkArgs {
            a_arg: TestModuleANewArg,
            b_arg: TestModuleBNewArg,
        };
        
        // 使用trait方法初始化
        fw.init(args).await.unwrap();
        //println!("Initialized framework");
        
        // 通过 framework 获取 TestModuleA 的 view
        let view = fw.a_view();
        
        // 验证 TestModuleA 已初始化
        assert!(*view.test_module_a().initialized.lock().unwrap());
        assert!(!*view.test_module_a().shutdown.lock().unwrap());
        
        // 验证 TestModuleB 已初始化
        assert!(*view.test_module_b().initialized.lock().unwrap());
        assert!(!*view.test_module_b().shutdown.lock().unwrap());
        
        //println!("TestModuleB name: {}", view.test_module_b().name());
        assert_eq!(view.test_module_b().name(), "TestModuleB");
        
        // 使用trait方法关闭
        fw.shutdown().await.unwrap();
        //println!("Shutdown framework");
        
        // 验证模块已关闭
        let view = fw.a_view();
        assert!(*view.test_module_a().shutdown.lock().unwrap());
        assert!(*view.test_module_b().shutdown.lock().unwrap());
    }
}

