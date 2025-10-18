// lib.rs (replacement)
use bincode;
use bytes::Bytes;
use futures::{Sink, SinkExt, Stream};
use hex;
use round_based::{Incoming, Outgoing};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// ======================
/// INCOMING TRANSPORT
/// ======================
pub struct TcpIncoming<M> {
    id: u64,
    framed: Framed<TcpStream, LengthDelimitedCodec>,
    _phantom: PhantomData<M>,
}

impl<M> TcpIncoming<M> {
    pub fn new(stream: TcpStream, id: u64) -> Self {
        Self {
            id,
            framed: Framed::new(stream, LengthDelimitedCodec::new()),
            _phantom: PhantomData,
        }
    }
}

impl<M> Stream for TcpIncoming<M>
where
    M: DeserializeOwned + Send + Unpin + 'static,
{
    type Item = Result<Incoming<M>, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Safe: we don't move `framed` out of the struct
        let this = unsafe { self.get_unchecked_mut() };
        match Pin::new(&mut this.framed).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                // bytes implements AsRef<[u8]>
                println!("[Receiver] Received {} bytes", bytes.len());

                match bincode::deserialize::<M>(bytes.as_ref()) {
                    Ok(msg) => {
                        let incoming = Incoming {
                            id: this.id,
                            sender: (this.id as u16).into(),
                            msg_type: round_based::MessageType::Broadcast,
                            msg,
                        };
                        Poll::Ready(Some(Ok(incoming)))
                    }
                    Err(e) => {
                        eprintln!("Failed to deserialize incoming message: {:?}", e);
                        Poll::Ready(Some(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("deserialize error: {}", e),
                        ))))
                    }
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// ======================
/// OUTGOING TRANSPORT
/// ======================
#[derive(Clone)]
pub struct TcpOutgoing<M> {
    id: u64,
    tx: UnboundedSender<Bytes>,
    _phantom: PhantomData<M>,
}

impl<M> TcpOutgoing<M> {
    pub fn new(stream: TcpStream, id: u64) -> Self {
        let (tx, rx) = unbounded_channel();

        // Create a framed writer that is owned by the sender task
        let framed_writer = Framed::new(stream, LengthDelimitedCodec::new());

        // spawn task that owns the framed writer and forwards messages from rx
        tokio::spawn(async move {
            run_sender(framed_writer, rx).await;
        });

        Self {
            id,
            tx,
            _phantom: PhantomData,
        }
    }
}

async fn run_sender(
    mut framed: Framed<TcpStream, LengthDelimitedCodec>,
    mut rx: UnboundedReceiver<Bytes>,
) {
    println!("sending the commitments [run_sender]");
    if let Ok(peer) = framed.get_ref().peer_addr() {
        println!("[Sender] Connected to {}", peer);
    }
    
    while let Some(msg) = rx.recv().await {
        println!("Message (hex): {}", hex::encode(&msg));
        println!("[Sender] Sent {} bytes", msg.len());
        // framed.send wants bytes that implement Into<bytes::Bytes>
        if let Err(e) = framed.send(msg).await {
            eprintln!("Failed to send message: {:?}", e);
            break;
        }
    }
}

impl<M> Sink<Outgoing<M>> for TcpOutgoing<M>
where
    M: Serialize + Send + 'static,
{
    type Error = std::io::Error;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Outgoing<M>) -> Result<(), Self::Error> {
        println!("sending the commitments on the connection [start_send]");
        let data = bincode::serialize(&item.msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        // use bytes::Bytes
        let _ = self.tx.send(Bytes::from(data));
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
