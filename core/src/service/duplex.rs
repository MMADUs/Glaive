use bytes::{BufMut, Bytes, BytesMut};
use std::error::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::service::service::{Service, ServiceType};
use crate::stream::stream::Stream;

#[derive(Debug)]
enum ProxyTask {
    Body(Option<Arc<Bytes>>, bool),
    Failed(Box<dyn Error>),
}

impl ProxyTask {
    fn is_end(&self) -> bool {
        match self {
            ProxyTask::Body(_, end) => *end,
            ProxyTask::Failed(_) => true,
        }
    }
}

struct Connection {
    stream: Stream,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

impl Connection {
    fn new(stream: Stream) -> Self {
        Self {
            stream,
            read_buf: BytesMut::with_capacity(8 * 1024),
            write_buf: BytesMut::with_capacity(8 * 1024),
        }
    }

    async fn read_data(&mut self) -> Result<Option<Arc<Bytes>>, Box<dyn Error>> {
        // Read from TCP stream into the buffer
        let bytes_read = self.stream.read_buf(&mut self.read_buf).await?;
        if bytes_read == 0 {
            return Ok(None); // EOF reached
        }

        // Split off the filled portion into a Bytes
        let data = self.read_buf.split().freeze();
        Ok(Some(Arc::new(data)))
    }

    async fn write_data(&mut self, data: Arc<Bytes>) -> Result<(), Box<dyn Error>> {
        self.write_buf.put_slice(&data);
        // Write the buffered data to the TCP stream
        self.stream.write_all(&self.write_buf).await?;
        self.write_buf.clear();
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        if !self.write_buf.is_empty() {
            self.stream.write_all(&self.write_buf).await?;
            self.write_buf.clear();
        }
        self.stream.flush().await?;
        Ok(())
    }
}

// handle request and io zero-copy bidirectionally
// with full duplex mechanism
impl<A: ServiceType + Send + Sync + 'static> Service<A> {
    async fn copy_bidirectional(
        &self,
        client_session: &mut Connection,
        server_session: &mut Connection,
    ) -> Result<(), Box<dyn Error>> {
        const BUFFER_SIZE: usize = 32;

        let (tx_to_server, rx_to_server) = mpsc::channel(BUFFER_SIZE);
        let (tx_to_client, rx_to_client) = mpsc::channel(BUFFER_SIZE);

        let result = tokio::try_join!(
            Self::copy_client_to_server(client_session, tx_to_server, rx_to_client),
            Self::copy_server_to_client(server_session, tx_to_client, rx_to_server)
        );

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn copy_client_to_server(
        client: &mut Connection,
        tx: mpsc::Sender<ProxyTask>,
        mut rx: mpsc::Receiver<ProxyTask>,
    ) -> Result<(), Box<dyn Error>> {
        // process flag
        // to determine wether which process is completed
        let mut request_done = false;
        let mut response_done = false;

        // duplex mode
        // running concurrently
        // this will keep running until the process flag finished
        while !request_done || !response_done {
            tokio::select! {
                // the first process that execute concurrently
                // read the buffer from client
                // process will keep running until request flag is done/finished
                result = client.read_data(), if !request_done => {
                    match result {
                        // processed to read the data from client
                        Ok(maybe_data) => {
                            // send the proxy task to the channel
                            // the task contains the buffer from client
                            // task will be consumed by another thread concurrently for writing to upstream
                            let is_end = maybe_data.is_none();
                            let _ = tx.send(ProxyTask::Body(maybe_data, is_end)).await;
                            request_done = is_end;
                        }
                        Err(e) => {
                            let _ = tx.send(ProxyTask::Failed(e)).await;
                            return Ok(());
                        }
                    }
                }

                // the last process that execute concurrently
                // received the task from the server buffer reader
                // task contains the response from server
                // process will keep running until response flag is done/finished
                maybe_task = rx.recv(), if !response_done => {
                    match maybe_task {
                        // if buffer exist from upstream server
                        // write data to client
                        // when writing finished, response is sent to client instantly.
                        Some(ProxyTask::Body(maybe_data, is_end)) => {
                            if let Some(data) = maybe_data {
                                client.write_data(data).await?;
                            }
                            if is_end {
                                client.flush().await?;
                            }
                            response_done = is_end;
                        }
                        // when the received task is Error
                        // return error
                        Some(ProxyTask::Failed(e)) => return Err(e),
                        None => response_done = true,
                    }
                }

                else => break,
            }
        }

        Ok(())
    }

    async fn copy_server_to_client(
        server: &mut Connection,
        tx: mpsc::Sender<ProxyTask>,
        mut rx: mpsc::Receiver<ProxyTask>,
    ) -> Result<(), Box<dyn Error>> {
        let mut request_done = false;
        let mut response_done = false;

        while !request_done || !response_done {
            tokio::select! {
                // the third process that execute concurrently
                // it is listening for response from upstream server
                // tries to read buffer from upstream server
                // process will keep runnign until response flag is done/finished
                result = server.read_data(), if !response_done => {
                    match result {
                        // processed to read data from upstream server
                        Ok(maybe_data) => {
                            // send the proxy task to the channel
                            // the task contains the buffer from upstream server
                            // task will be consumed by another thread concurrently for writing to downstream client
                            let is_end = maybe_data.is_none();
                            let _ = tx.send(ProxyTask::Body(maybe_data, is_end)).await;
                            response_done = is_end;
                        }
                        Err(e) => {
                            let _ = tx.send(ProxyTask::Failed(e)).await;
                            return Ok(());
                        }
                    }
                }

                // the second process that execute concurrently
                // received the task from the client buffer reader
                // task contains the client buffer
                // process will keep running until request flag is done/finished
                maybe_task = rx.recv(), if !request_done => {
                    match maybe_task {
                        // receive the task with buffer
                        Some(ProxyTask::Body(maybe_data, is_end)) => {
                            // perform a write operation to upstream server
                            if let Some(data) = maybe_data {
                                server.write_data(data).await?;
                            }
                            if is_end {
                                server.flush().await?;
                            }
                            request_done = is_end;
                        }
                        // when the received task is Error
                        // return error
                        Some(ProxyTask::Failed(e)) => return Err(e),
                        None => request_done = true,
                    }
                }

                else => break,
            }
        }

        Ok(())
    }
}
