use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::hash::Hash;

use lru::LruCache;
use parking_lot::RwLock;
use thread_local::ThreadLocal;
use tokio::sync::Notify;

// the connection node represents the connection value in lru
// its the value type for the cache
// the value consist the connection metadata & the notifier
// notifier is used to notify the main connection pool when the connection in lru gets evicted
pub struct ConnectionNode<M> {
    pub removal_notifier: Arc<Notify>,
    pub metadata: M,
}

impl<M> ConnectionNode<M> {
    // create a new conenction node
    pub fn new(connection_metadata: M) -> Self {
        ConnectionNode {
            removal_notifier: Arc::new(Notify::new()),
            metadata: connection_metadata,
        }
    }

    // this method is used to notify during removal
    // the notification triggers when the node is removed
    pub fn notify_removal(&self) {
        self.removal_notifier.notify_one();
    }
}

// the connection lru is the main lru storage
// it stores all the connection metadata in lru cache
// lru were able to evict the oldest connection when it hits the size limit
// the generics U: type for connection unique id
// the generics M: type for connection metadata
pub struct ConnectionLru<U, M>
where
    U: Send,
    M: Send,
{
    // the data store for all lru data
    lru_store: RwLock<ThreadLocal<RefCell<LruCache<U, ConnectionNode<M>>>>>,
    // stores the lru maxium capacity size
    size_capacity: usize,
    // the drain status flag is used when draining is on process
    // used atomic bool for thread safe bool
    drain_status_flag: AtomicBool,
}

impl<U, M> ConnectionLru<U, M>
where
    U: Hash + Eq + Send,
    M: Send,
{
    // new lru storage instance
    // by default the drain status flag is false
    pub fn new(lru_size: usize) -> Self {
        ConnectionLru {
            lru_store: RwLock::new(ThreadLocal::new()),
            size_capacity: lru_size,
            drain_status_flag: AtomicBool::new(false),
        }
    }

    // add a new connection to the lru
    // the new connection will create a new node for the metadata
    pub fn add_new_connection(
        &self, 
        connection_unique_id: U, 
        connection_metadata: M,
    ) -> (Arc<Notify>, Option<M>) {
        // create new node with metadata
        let connection_node = ConnectionNode::new(connection_metadata);
        // clone the notifier to be shared across connection pool
        let notifier = connection_node.removal_notifier.clone();
        // insert key and node to the lru store
        let new_metadata = self.insert_connection(connection_unique_id, connection_node);
        // return the new connection metadata and the removal notifier
        (notifier, new_metadata)
    }

    // insert a node in and return the meta of the replaced node
    // any node with existed key in lru, will be updated to the latest inserted value
    pub fn insert_connection(&self, connection_unique_id: U, connection_node: ConnectionNode<M>) -> Option<M> {
        // check for the drain status flag
        // if the drain is true, means the draining process is currently running
        if self.drain_status_flag.load(Relaxed) {
            // if the draining process is running
            // reject the node insert process
            // and notify the node to be removed
            connection_node.notify_removal();
            return None;
        }
        // acquire read lock
        let lru = self.lru_store.read();
        // get the mutable lru cache store
        let lru_cache = &mut *(lru
            .get_or(|| RefCell::new(LruCache::unbounded()))
            .borrow_mut());
        // inserting key and the connection node
        // if the key already exist in the lru, it will update with the latest inserted value
        lru_cache.put(connection_unique_id, connection_node);
        // checks the total of the connection inside the lru store
        // if the total connection is more than the capacity
        // we pop out the recently used connection
        if lru_cache.len() > self.size_capacity {
            match lru_cache.pop_lru() {
                Some((_, connection)) => {
                    // notify removal since connection is no longer in lru
                    connection.notify_removal();
                    // return the popped connection metadata
                    return Some(connection.metadata);
                }
                None => return None,
            }
        }
        None
    }

    // used to pop out connection from the lru
    pub fn pop_connection(&self, connection_unique_id: &U) -> Option<ConnectionNode<M>> {
        // acquire read lock
        let lru = self.lru_store.read();
        // get the mutable lru cache store
        let lru_cache = &mut *(lru
            .get_or(|| RefCell::new(LruCache::unbounded()))
            .borrow_mut());
        // pop the connection from lru and return it straightaway
        lru_cache.pop(connection_unique_id)
    }

    // used to drain all the connections inside lru
    // this will entirely clear all connections
    pub fn drain_connections(&self) {
        // set the drain status flag to true
        // this was used to prevent any new inserted connection while draining is in process
        self.drain_status_flag.store(true, Relaxed);
        // acquire write lock
        let mut lru = self.lru_store.write();
        // make the lru cache iterable and mutable
        let lru_cache_iter = lru.iter_mut();
        for lru_cache in lru_cache_iter {
            // get all connections as mutable objects
            let mut connections = lru_cache.borrow_mut();
            // iterate over all connections
            // used to broadcast to all connection in the connection pool
            // notify a removal events
            for (_, item) in connections.iter() {
                item.notify_removal();
            }
            // clear all connection after broadcasting
            connections.clear();
        }
    }
}
