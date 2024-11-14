use std::sync::Arc;
use std::{collections::HashMap, time::Duration};

use super::lru::ConnectionLru;
use crossbeam_queue::ArrayQueue;
use parking_lot::{Mutex, RwLock};
use tokio::sync::{oneshot, Notify};

// used for every connection
// every connection have their own unique id
// used for network pooling
pub type ConnectionID = i32;

// used for a group of connection
// the connection group id can held many connection unique id
// used for service pooling
pub type ConnectionGroupID = u64;

// connection metadata is used to store connection information
#[derive(Clone)]
pub struct ConnectionMetadata {
    // determine on which connection peer this belongs to
    pub group_id: ConnectionGroupID,
    // unique id for every connection
    pub unique_id: ConnectionID,
}

impl ConnectionMetadata {
    // new connection metadata
    pub fn new(group_id: ConnectionGroupID, unique_id: ConnectionID) -> Self {
        ConnectionMetadata {
            group_id,
            unique_id,
        }
    }
}

// the connection notifier is wrapping the actual connection
// its used to notify the current connection when its being picked up by request
struct ConnectionNotifier<S> {
    pub pickup_notifier: oneshot::Sender<bool>,
    pub connection: S,
}

impl<S> ConnectionNotifier<S> {
    // new connection notifier
    pub fn new(notifier: oneshot::Sender<bool>, connection: S) -> Self {
        ConnectionNotifier {
            pickup_notifier: notifier,
            connection,
        }
    }

    // used to pickup the current connection
    // also notify the connection being picked up
    pub fn pick_up_connection(self) -> S {
        let _ = self.pickup_notifier.send(true);
        self.connection
    }
}

// the connection node is used to held many connections
// the node uses 2 connection store
// the connections field: used for the main list of connections
// the hot queue: used for hot queue connections
pub struct ConnectionNode<T> {
    // primary connection list
    connections: Mutex<HashMap<ConnectionID, T>>,
    // seconday connection list
    hot_queue: ArrayQueue<(ConnectionID, T)>,
    // used for the hot queue remove lock
    hot_queue_remove_lock: Mutex<()>,
}

// the size for the hot_queue to hold the connection
// TODO: make this configurable soon
const HOT_QUEUE_SIZE: usize = 16;

// remember, any lock that is acquired in the functions
// will be unlocked when the function ends.
impl<T> ConnectionNode<T> {
    // new connection node
    pub fn new() -> Self {
        ConnectionNode {
            connections: Mutex::new(HashMap::new()),
            hot_queue: ArrayQueue::new(HOT_QUEUE_SIZE),
            hot_queue_remove_lock: Mutex::new(()),
        }
    }

    // this is used to pick up any available connection inside the node
    // this returns the connection_id and the connection itself
    pub fn get_available_connection(&self) -> Option<(ConnectionID, T)> {
        // lookup for some connection in the hot queue
        let hot_connection = self.hot_queue.pop();
        if hot_connection.is_some() {
            return hot_connection;
        }
        // otherwise, find on the connections list
        // acquire a lock to get all connections
        let mut connection_list = self.connections.lock();
        let connection_id = match connection_list.iter().next() {
            Some((conn_id, _)) => *conn_id,
            None => return None,
        };
        // pop out the connection from the list
        if let Some(connection) = connection_list.remove(&connection_id) {
            return Some((connection_id, connection));
        } else {
            None
        }
    }

    // used to insert a new connection to the node
    // returns nothing
    pub fn add_new_connection(&self, connection_id: ConnectionID, connection: T) {
        // we first try to insert the connection to hot queue
        if let Err(node) = self.hot_queue.push((connection_id, connection)) {
            // when inserting to hot queue fails, then we can start inserting to the main connections list
            // acquire a lock to get all connections
            let mut connection_list = self.connections.lock();
            // insert the id and the connection to list
            connection_list.insert(node.0, node.1);
        }
    }

    // used to remove connection from the node
    // it returns the removed connection if exist
    pub fn remove_connection(&self, connection_id: ConnectionID) -> Option<T> {
        // acquire a lock to get all connections
        let mut connection_list = self.connections.lock();
        // remove a connection given by connection id
        let removed = connection_list.remove(&connection_id);
        if removed.is_some() {
            return removed;
        }
        // if connection is not in the list, find on the hot queue
        // we acquire lock on the hot queue lock here
        let _ = self.hot_queue_remove_lock.lock();
        let total_queue = self.hot_queue.len();
        // find the connection in hot queue
        for _ in 0..total_queue {
            // try to pop every connection in the hot_queue
            // if connection found then return it, otherwise
            // insert back to connection list
            if let Some((hot_queue_connection_id, hot_queue_connection)) = self.hot_queue.pop() {
                if connection_id == hot_queue_connection_id {
                    // this is the item we are looking for
                    // return the conenction
                    return Some(hot_queue_connection);
                } else {
                    // this is when we didnt find the connection by id
                    // so insert back to connection list
                    self.add_new_connection(hot_queue_connection_id, hot_queue_connection);
                }
            } else {
                return None;
            }
        }
        None
    }
}

// the main connection pool that helds the entire system connections.
// it holds a reusable connection inside the connection nodes
// every available connection inside this pool will be picked up by a client request
pub struct ConnectionPool<S> {
    // holds the entire connection
    pool: RwLock<HashMap<ConnectionGroupID, Arc<ConnectionNode<ConnectionNotifier<S>>>>>,
    // connection lru manager
    lru: ConnectionLru<ConnectionID, ConnectionMetadata>,
}

// PUBLIC METHODS
// connection pool implementation
impl<S> ConnectionPool<S> {
    // new connection pool
    // the size is used to determine the connection pool capacity size for storing connection node
    pub fn new(pool_node_size: usize) -> Self {
        ConnectionPool {
            pool: RwLock::new(HashMap::with_capacity(pool_node_size)),
            lru: ConnectionLru::new(pool_node_size),
        }
    }

    // used to open a new connection with the given metadata
    // this method returns 2 notifier
    // 1. the closed connection notifier
    // 2. the picked up connection notifier
    pub fn add_connection(
        &self,
        metadata: &ConnectionMetadata,
        connection: S,
    ) -> (Arc<Notify>, oneshot::Receiver<bool>) {
        self.add_connection_to_pool(metadata, connection)
    }

    // used to find a connection in pool with the given connection group id
    // this will return Some connection, so theres a chance connection does not exist
    pub fn find_connection(&self, connection_group_id: ConnectionGroupID) -> Option<S> {
        self.get_connection_from_pool(connection_group_id)
    }

    // used to close a connection in pool with the given metadata
    // returns nothing
    pub fn close_connection(&self, metadata: &ConnectionMetadata) {
        self.remove_connection(metadata);
    }

    // this special method can be applied to a new opened connection
    // using this method is optional
    // when using this method, it gives timeout to connection in the pool
    // the timeout is applies by the given metadata
    // when it reach the given timeout duration, the connection will be closed immediately.
    pub async fn connection_idle_timeout(
        &self,
        metadata: &ConnectionMetadata,
        timeout: Duration,
        closed_connection_notifier: Arc<Notify>,
        connection_pickup_notification: oneshot::Receiver<bool>,
    ) {
        tokio::select! {
            biased;
            // connection picked up event
            _ = connection_pickup_notification => {
                println!("idle connection is being picked up");
            },
            // connection evicted event
            _ = closed_connection_notifier.notified() => {
                println!("idle connection is being evicted");
            }
            // timeout reached
            _ = tokio::time::sleep(timeout) => {
                println!("idle connection reached timeout, connection closed");
                // remove connection when timeout reached with the metadata
                self.remove_connection(metadata);
            },
        };
    }
}

// PRIVATE METHODS
// connection pool implementation
impl<S> ConnectionPool<S> {
    // used to get a connecion node based on the given connection group id
    // if node does not exist in connectionn pool, the new empty node is added to pool.
    // this returns Arc connection node
    fn get_connection_node(
        &self,
        connection_group_id: ConnectionGroupID,
    ) -> Arc<ConnectionNode<ConnectionNotifier<S>>> {
        // made a new block to encapsulate lock operation
        {
            // acquire lock to read
            let pool = self.pool.read();
            // find node by connection group id, if exist return it
            if let Some(node) = pool.get(&connection_group_id) {
                return (*node).clone();
            }
        } // the read lock will be unlock by the end of this block
          // made another block to encapsulate lock operation
        {
            // acquired lock to write
            let mut pool = self.pool.write();
            // checks in case another process already added it
            if let Some(node) = pool.get(&connection_group_id) {
                return (*node).clone();
            }
            // when node does not exist during lookup, add a new empty node to the pool
            let new_node = Arc::new(ConnectionNode::new());
            pool.insert(connection_group_id, new_node.clone());
            // return connection node
            new_node
        }
    }

    // used to get any available connection by the given connection group id
    // it will search the connection from a node, and find any available connection inside the node.
    // this return Some connection, there is a chance where it does not exist.
    fn get_connection_from_pool(&self, connection_group_id: ConnectionGroupID) -> Option<S> {
        // find connection node with connection group id
        // the process are encapsulated for lock operation
        let connection_node = {
            // acquire read lock
            let pool = self.pool.read();
            // return connection node if exist
            match pool.get(&connection_group_id) {
                Some(node) => (*node).clone(),
                None => return None,
            }
        }; // unlock here
           // after connection node was found, get any available connection within the node
        if let Some((connection_unique_id, connection)) = connection_node.get_available_connection()
        {
            // pop connection from lru
            // because we are picking up the connection
            self.lru.pop_connection(&connection_unique_id);
            // return the connection
            Some(connection.pick_up_connection())
        } else {
            None
        }
    }

    // used to remove a connection by the given connection metadata
    // note: this is only removing connection from pool, not lru
    // lookup the node by the given metadata group id
    // remove the connection by the metadata unique id
    // returns nothing
    fn remove_connection_from_pool(&self, connection_meta: &ConnectionMetadata) {
        // find connection node
        // the process are encapsulated for lock operation
        let connection_node = {
            // acquire read lock
            let pool = self.pool.read();
            match pool.get(&connection_meta.group_id) {
                Some(node) => (*node).clone(),
                None => {
                    // if we did not find the node, end the function
                    // just give some warning here
                    println!(
                        "node not found for meta with group id: {:?}",
                        connection_meta.group_id
                    );
                    return;
                }
            }
        }; // unlock here
           // if connection node was found, remove it from the pool
        connection_node.remove_connection(connection_meta.unique_id);
    }

    // used to close connection by the given connection metadata
    // remove both connection from pool and lru
    // returns nothing
    fn remove_connection(&self, metadata: &ConnectionMetadata) {
        // remove both from pool and lru
        self.remove_connection_from_pool(metadata);
        self.lru.pop_connection(&metadata.unique_id);
    }

    // used to register a new conenction to the connection pool
    // insert both to connection pool and lru
    // 1. the closed connection notifier
    // 2. the picked up connection notifier
    fn add_connection_to_pool(
        &self,
        metadata: &ConnectionMetadata,
        connection: S,
    ) -> (Arc<Notify>, oneshot::Receiver<bool>) {
        // create a new connection in lru
        let (closed_connection_notifier, replaced) = self
            .lru
            .add_new_connection(metadata.unique_id, metadata.clone());
        // if exist, remove the connection from pool
        if let Some(meta) = replaced {
            self.remove_connection_from_pool(&meta);
        }
        // get connection node
        let connection_node = self.get_connection_node(metadata.group_id);
        // we made a new oneshot channel
        // upon creating channel, it returns 2 type
        //
        // connection pickup notifier = notifies connection pickups
        // connection pickup notification = receive the connection pickups notification
        let (connection_pickup_notifier, connection_pickup_notification) = oneshot::channel();
        // create new notifier for the new connection
        let connection_notifier = ConnectionNotifier::new(connection_pickup_notifier, connection);
        // add new connection to the connection pool
        connection_node.add_new_connection(metadata.unique_id, connection_notifier);
        // returns the notifier listeners
        // this will be listened by another process
        (closed_connection_notifier, connection_pickup_notification)
    }
}
