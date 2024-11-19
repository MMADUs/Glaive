use bytes::{BufMut, Bytes, BytesMut};
use std::error::Error;
use std::sync::Arc;
// use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::service::buffer::BufferSession;
use crate::service::service::{Service, ServiceType};
use crate::stream::stream::Stream;

#[derive(Debug)]
enum ProxyTask {
    Body(Option<Arc<Bytes>>, bool),
    Failed(Box<dyn Error + Send + Sync>),
}

impl ProxyTask {
    fn is_end(&self) -> bool {
        match self {
            ProxyTask::Body(_, end) => *end,
            ProxyTask::Failed(_) => true,
        }
    }
}

// struct Connection {
//     stream: Stream,
//     read_buf: BytesMut,
//     write_buf: BytesMut,
// }
//
// impl Connection {
//     fn new(stream: Stream) -> Self {
//         Self {
//             stream,
//             read_buf: BytesMut::with_capacity(1024 * 64),
//             write_buf: BytesMut::with_capacity(1024 * 64),
//         }
//     }
//
//     async fn read_data(&mut self) -> Result<Option<Arc<Bytes>>, Box<dyn Error>> {
//         // Read from TCP stream into the buffer
//         let bytes_read = self.stream.read_buf(&mut self.read_buf).await?;
//         if bytes_read == 0 {
//             return Ok(None); // EOF reached
//         }
//
//         // Split off the filled portion into a Bytes
//         let data = self.read_buf.split().freeze();
//         Ok(Some(Arc::new(data)))
//     }
//
//     async fn write_data(&mut self, data: Arc<Bytes>) -> Result<(), Box<dyn Error>> {
//         self.write_buf.put_slice(&data);
//         // Write the buffered data to the TCP stream
//         self.stream.write_all(&self.write_buf).await?;
//         self.write_buf.clear();
//         Ok(())
//     }
//
//     async fn flush(&mut self) -> Result<(), Box<dyn Error>> {
//         if !self.write_buf.is_empty() {
//             self.stream.write_all(&self.write_buf).await?;
//             self.write_buf.clear();
//         }
//         self.stream.flush().await?;
//         Ok(())
//     }
// }

const BUFFER_SIZE: usize = 32;

// // handle request and io zero-copy bidirectionally
// // with full duplex mechanism
// impl<A: ServiceType> Service<A> {
//     // the main operation that does io copy bidirectionally
//     // split the session buffer into 2 diffrent concurrent task
//     pub async fn copy_bidirectional(
//         &self,
//         client_session: &mut SessionBuffer,
//         server_session: &mut SessionBuffer,
//     ) -> Result<(), Option<Box<dyn Error + Send + Sync>>> {
//         // split stream buffer into 2 task
//         let (tx_to_server, rx_to_server) = mpsc::channel(BUFFER_SIZE);
//         let (tx_to_client, rx_to_client) = mpsc::channel(BUFFER_SIZE);
//
//         println!("copying stream...");
//         // handle both concurrently
//         let result = tokio::try_join!(
//             Self::handle_downstream(client_session, tx_to_server, rx_to_client),
//             Self::handle_upstream(server_session, tx_to_client, rx_to_server)
//         );
//
//         match result {
//             Ok(_) => Ok(()),
//             Err(e) => Err(Some(e)),
//         }
//     }
//
//     // function runs concurrently
//     // the operation for copying buffer from client to server
//     async fn handle_downstream(
//         client: &mut SessionBuffer,
//         tx: mpsc::Sender<ProxyTask>,
//         mut rx: mpsc::Receiver<ProxyTask>,
//     ) -> Result<(), Box<dyn Error + Send + Sync>> {
//         // process flag
//         // to determine wether which process is completed
//         let mut request_done = false;
//         let mut response_done = false;
//
//         // duplex mode
//         // running concurrently
//         // this will keep running until the process flag finished
//         while !request_done || !response_done {
//             tokio::select! {
//                 // the first process that execute concurrently
//                 // read the buffer from client
//                 // process will keep running until request flag is done/finished
//                 result = client.read_body(), if !request_done => {
//                     match result {
//                         // processed to read the data from client
//                         Ok(maybe_data) => {
//                             println!("read from client");
//                             // send the proxy task to the channel
//                             // the task contains the buffer from client
//                             // task will be consumed by another thread concurrently for writing to upstream
//                             let is_end = maybe_data.is_none();
//                             tx.send(ProxyTask::Body(maybe_data, is_end)).await?;
//                             request_done = is_end;
//                         }
//                         Err(e) => {
//                             let error: Box<dyn Error + Send + Sync> = Box::new(e);
//                             tx.send(ProxyTask::Failed(error)).await?;
//                             return Ok(());
//                         }
//                     }
//                 }
//
//                 // the last process that execute concurrently
//                 // received the task from the server buffer reader
//                 // task contains the response from server
//                 // process will keep running until response flag is done/finished
//                 maybe_task = rx.recv(), if !response_done => {
//                     match maybe_task {
//                         // if buffer exist from upstream server
//                         // write data to client
//                         // when writing finished, response is sent to client instantly.
//                         Some(ProxyTask::Body(maybe_data, is_end)) => {
//                             println!("send to client");
//                             if let Some(data) = maybe_data {
//                                 client.write_body(data).await?;
//                             }
//                             if is_end {
//                                 client.flush().await?;
//                             }
//                             response_done = is_end;
//                         }
//                         // when the received task is Error
//                         // return error
//                         Some(ProxyTask::Failed(e)) => return Err(e),
//                         None => response_done = true,
//                     }
//                 }
//
//                 // this should not be reach at any point
//                 else => break,
//             }
//         }
//
//         Ok(())
//     }
//
//     // function runs concurrently
//     // the operation for copying buffer from server to client
//     async fn handle_upstream(
//         server: &mut SessionBuffer,
//         tx: mpsc::Sender<ProxyTask>,
//         mut rx: mpsc::Receiver<ProxyTask>,
//     ) -> Result<(), Box<dyn Error + Send + Sync>> {
//         let mut request_done = false;
//         let mut response_done = false;
//
//         while !request_done || !response_done {
//             tokio::select! {
//                 // the third process that execute concurrently
//                 // it is listening for response from upstream server
//                 // tries to read buffer from upstream server
//                 // process will keep runnign until response flag is done/finished
//                 result = server.read_body(), if !response_done => {
//                     match result {
//                         // processed to read data from upstream server
//                         Ok(maybe_data) => {
//                             println!("read response");
//                             // send the proxy task to the channel
//                             // the task contains the buffer from upstream server
//                             // task will be consumed by another thread concurrently for writing to downstream client
//                             let is_end = maybe_data.is_none();
//                             tx.send(ProxyTask::Body(maybe_data, is_end)).await?;
//                             response_done = is_end;
//                         }
//                         Err(e) => {
//                             let error: Box<dyn Error + Send + Sync> = Box::new(e);
//                             tx.send(ProxyTask::Failed(error)).await?;
//                             return Ok(());
//                         }
//                     }
//                 }
//
//                 // the second process that execute concurrently
//                 // received the task from the client buffer reader
//                 // task contains the client buffer
//                 // process will keep running until request flag is done/finished
//                 maybe_task = rx.recv(), if !request_done => {
//                     match maybe_task {
//                         // receive the task with buffer
//                         Some(ProxyTask::Body(maybe_data, is_end)) => {
//                             println!("send to backend");
//                             // perform a write operation to upstream server
//                             if let Some(data) = maybe_data {
//                                 server.write_body(data).await?;
//                             }
//                             if is_end {
//                                 server.flush().await?;
//                             }
//                             request_done = is_end;
//                         }
//                         // when the received task is Error
//                         // return error
//                         Some(ProxyTask::Failed(e)) => return Err(e),
//                         None => request_done = true,
//                     }
//                 }
//
//                 // this should not be reach at any point
//                 else => break,
//             }
//         }
//
//         Ok(())
//     }
// }
