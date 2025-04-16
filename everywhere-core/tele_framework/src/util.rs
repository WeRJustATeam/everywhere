use std::{
    fmt::Debug,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll},
};

use parking_lot::MutexGuard;
#[cfg(test)]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

use tokio::task::JoinHandle;

#[cfg(test)]
pub fn test_tracing_start() {
    let my_filter = tracing_subscriber::filter::filter_fn(|v| {
        if let Some(mp) = v.module_path() {
            if mp.contains("async_raft") {
                return false;
            }
            if mp.contains("hyper") {
                return false;
            }
        }
        v.level() != &tracing::Level::TRACE
    });
    let my_layer = tracing_subscriber::fmt::layer();
    let _ = tracing_subscriber::registry()
        .with(my_layer.with_filter(my_filter))
        .try_init();
}

pub struct JoinHandleWrapper(pub JoinHandle<()>);

impl Future for JoinHandleWrapper {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(()),
            Poll::Ready(Err(e)) => {
                eprintln!("Task error: {}", e);
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub enum WithBind<'a, T> {
    MutexGuard(MutexGuard<'a, T>),
    MutexGuardOpt(MutexGuard<'a, Option<T>>),
}

impl<T> WithBind<'_, T> {
    pub fn option_mut(&mut self) -> &mut Option<T> {
        match self {
            Self::MutexGuard(_) => unreachable!(),
            Self::MutexGuardOpt(g) => &mut *g,
        }
    }
}

impl<T> Deref for WithBind<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::MutexGuard(g) => g,
            Self::MutexGuardOpt(g) => g.as_ref().unwrap(),
        }
    }
}

impl<T> DerefMut for WithBind<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::MutexGuard(g) => g,
            Self::MutexGuardOpt(g) => g.as_mut().unwrap(),
        }
    }
}

pub struct StrUnsafeRef(usize, usize);

impl StrUnsafeRef {
    pub fn new(str: &str) -> StrUnsafeRef {
        StrUnsafeRef(str.as_ptr() as usize, str.len())
    }
    pub fn str<'a>(&self) -> &'a str {
        std::str::from_utf8(unsafe { std::slice::from_raw_parts(self.0 as *const u8, self.1) })
            .unwrap()
    }
}

pub struct TryUtf8VecU8(pub Vec<u8>);

impl Debug for TryUtf8VecU8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let res = std::str::from_utf8(&self.0);
        match res {
            Ok(str) => write!(f, "{}", str),
            Err(_) => write!(f, "{:?}", &self.0),
        }
    }
}

struct FutureWrapper<F: Future> {
    fut: Pin<Box<F>>,
}

impl<F> std::future::Future for FutureWrapper<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.fut.as_mut().poll(cx)
    }
}

unsafe impl<F> Send for FutureWrapper<F> where F: Future {}

pub struct SendNonNull<T>(pub NonNull<T>);
unsafe impl<T> Send for SendNonNull<T> {}

pub fn call_async_from_sync<Fut>(fut: Fut) -> Fut::Output
where
    Fut: std::future::Future + 'static,
    Fut::Output: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    let fut = async move {
        match tx.send(fut.await) {
            Ok(res) => res,
            Err(_) => {
                panic!("oneshot channel closed which should not happen")
            }
        }
    };
    let _ = tokio::task::spawn(FutureWrapper { fut: Box::pin(fut) });

    rx.blocking_recv().unwrap()
}

pub unsafe fn non_null<T>(v: &T) -> std::ptr::NonNull<T> {
    let ptr = v as *const T as *mut T;
    let non_null = std::ptr::NonNull::new_unchecked(ptr);
    non_null
}
