use bincode;
use bytes::Bytes;
use futures::{Sink, SinkExt, Stream};
use hex;
use round_based::{Incoming, Outgoing};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

pub mod keygen;
pub mod sign;
pub mod config;

#[derive(Serialize, Deserialize, Debug)]
enum MsgKind {
    Broadcast,
    P2P,
}

// for BROADCAST and P2P
#[derive(Serialize, Deserialize, Debug)]
struct WireMessage<M> {
    kind: MsgKind,
    recipient: Option<u16>, // only Some for P2P
    msg: M,
}

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
        println!("incoming_new");
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

                match bincode::deserialize::<WireMessage<M>>(bytes.as_ref()) {
                    Ok(wire_msg) => {
                        println!("constructing incoming msg: poll_next: incoming");

                        let msg_type = match wire_msg.kind {
                            MsgKind::Broadcast => round_based::MessageType::Broadcast,
                            MsgKind::P2P => round_based::MessageType::P2P,
                        };

                        let incoming = Incoming {
                            id: this.id,
                            sender: if this.id == 0 { 1 } else { 0 },
                            msg_type,
                            msg: wire_msg.msg,
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
        println!("outgoing_new");
        // Create a framed writer that is owned by the sender task
        let framed_writer = Framed::new(stream, LengthDelimitedCodec::new());

        // spawn task that owns the framed writer and forwards messages from rx
        tokio::spawn(async move {
            println!("tokio: spwan - run_sender");
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
        if let Err(e) = framed.send(msg).await {
            eprintln!("Failed to send message: {:?}", e);
            break;
        }
        if let Err(e) = framed.flush().await {
            eprintln!("Flush failed: {:?}", e);
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
        println!("poll_ready");
        Poll::Ready(Ok(()))
    }
    
    fn start_send(self: Pin<&mut Self>, item: Outgoing<M>) -> Result<(), Self::Error> {
        println!("sending the commitments on the connection [start_send]");

        let (kind, recipient) = match &item.recipient {
            round_based::MessageDestination::AllParties => (MsgKind::Broadcast, None),
            round_based::MessageDestination::OneParty(peer_id) => (MsgKind::P2P, Some(*peer_id)),
        };

        // wrap in our serializable container
        let wire_msg = WireMessage {
            kind,
            recipient,
            msg: item.msg,
        };

        let data = bincode::serialize(&wire_msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let _ = self.tx.send(Bytes::from(data));
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        println!("poll_flush");
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        println!("poll_close");
        Poll::Ready(Ok(()))
    }
}
