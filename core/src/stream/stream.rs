use tokio::io::{AsyncRead, AsyncWrite};
use std::any::Any;

use crate::stream::traits::UniqueID;

// the master trait used as implementation rules
pub trait StreamRules
    : AsyncRead
    + AsyncWrite
    + UniqueID
    + Unpin
    + Send
    + Sync
{
    // helper to cast as the reference of the concrete type
    fn as_any(&self) -> &dyn Any;
    // helper to cast back of the concrete type
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

// generics implementation for stream rules master trait
impl<T
    : AsyncRead
    + AsyncWrite
    + UniqueID
    + Unpin
    + Send
    + Sync,
> StreamRules for T
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

// the type for connection stream
// wrapped in box as dynamic types
// accepting any type that followed the master trait rules
pub type Stream = Box<dyn StreamRules>;
