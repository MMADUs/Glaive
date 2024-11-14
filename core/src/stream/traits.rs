use crate::pool::pool::ConnectionID;

pub trait UniqueID {
    fn get_unique_id(&self) -> ConnectionID;
}
