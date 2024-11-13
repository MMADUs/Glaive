use tokio::io::{AsyncRead, AsyncWrite};
use std::any::Any;

use crate::stream::traits::UniqueID;

/// The abstraction of transport layer IO
pub trait IO
    : AsyncRead
    + AsyncWrite
    + UniqueID
    + Unpin
    + Send
    + Sync
{
    /// helper to cast as the reference of the concrete type
    fn as_any(&self) -> &dyn Any;
    /// helper to cast back of the concrete type
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T
    : AsyncRead
    + AsyncWrite
    + UniqueID
    + Unpin
    + Send
    + Sync,
> IO for T
where
    T: 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// The type of any established transport layer connection
pub type Stream = Box<dyn IO>;
