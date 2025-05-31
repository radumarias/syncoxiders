use iroh::endpoint::{Accept, Connection};
use iroh::{Endpoint, NodeId};
use iroh::protocol::{ProtocolHandler, Router};
use tokio::sync::broadcast;
use anyhow::Result;
use async_channel::Sender;
use log::info;
use n0_future::boxed::BoxFuture;
use n0_future::future::Boxed;
use n0_future::{task, Stream};
use serde::{Deserialize, Serialize};

pub struct EchoNode{
    router: Router,
    accept_events: broadcast::Sender<AcceptEvent>,
}

impl EchoNode {
    pub async fn spawn() -> Result<Self> {

        let endpoint = Endpoint::builder().discovery_n0().alpns(vec![Echo::ALPN.to_vec()]).bind().await?;
        let (event_sender, _event_receiver) = broadcast::channel(128);
        let echo = Echo::new(event_sender.clone());
        let router = Router::builder(endpoint)
            .accept(Echo::ALPN, echo)
            .spawn();
        Ok(Self { router, accept_events: event_sender })


    }

    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    pub fn connect(
        &self,
        node_id: NodeId,
        payload: String
    ) -> impl Stream<Item = ConnectEvent> + Unpin {

        let (event_sender, event_receiver) = async_channel::bounded(16);
        let endpoint = self.router.endpoint().clone();
        task::spawn(async move {
            let res = connect(&endpoint, node_id, payload, event_sender.clone()).await;
            let error = res.as_ref().err().map(|e| e.to_string());
            event_sender.send(ConnectEvent::Closed {error}).await.ok();
        });
        Box::pin(event_receiver)
    }
}

#[derive(Debug, Clone , Serialize, Deserialize)]
#[serde(tag="type", rename_all = "camelCase")]
pub enum ConnectEvent {
    Connected,
    Sent {bytes_sent: u64},
    Received {bytes_received: u64},
    Closed {error: Option<String>}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="type", rename_all = "camelCase")]
pub enum AcceptEvent {

    Accepted {
        node_id: NodeId,
    },
    Echoed {
        node_id: NodeId,
        bytes_sent: u64
    },
    Closed {
        node_id: NodeId,
        error: Option<String>
    }
}

#[derive(Debug, Clone)]
pub struct Echo{
    event_sender: broadcast::Sender<AcceptEvent>
}

impl Echo{
    pub const ALPN: &[u8] = b"iroh/example-browser-echo/0";
    pub fn new(event_sender: broadcast::Sender<AcceptEvent>) -> Self {

        Self { event_sender }

    }
}

impl Echo {
    async fn handle_connection(self, connection: Connection) -> Result<()> {

        let node_id  = connection.remote_node_id()?;
        self.event_sender.send(AcceptEvent::Accepted {node_id }).ok();
        let res = self.handle_connection_0(&connection).await;
        let error = res.as_ref().err().map(|err| err.to_string());
        self.event_sender.send(AcceptEvent::Closed {node_id, error}).ok();
        res


    }

    async fn handle_connection_0(&self, connection: &Connection) -> Result<()> {

        let node_id = connection.remote_node_id()?;
        info!("Accepted connection from {}", node_id);

        let (mut send, mut recv) = connection.accept_bi().await?;

        //Echo any bytes received
        let bytes_sent = tokio::io::copy(&mut recv, &mut send).await?;

        info!("Copied over {bytes_sent} byte(s)");

        self.event_sender.send(AcceptEvent::Echoed {node_id, bytes_sent}).ok();

        send.finish()?;
        connection.closed().await;
        Ok(())


    }
}

impl ProtocolHandler for Echo{
    fn accept(&self, connection: Connection) -> BoxFuture<Result<()>> {
        Box::pin(self.clone().handle_connection(connection))
    }
}

async fn connect(
    endpoint: &Endpoint,
    node_id: NodeId,
    payload: String,
    event_sender: Sender<ConnectEvent>
) -> Result<()>{

    let connection = endpoint.connect(node_id, Echo::ALPN).await?;
    event_sender.send(ConnectEvent::Connected).await?;
    let (mut send_stream , mut recv_stream) = connection.open_bi().await?;
    let event_sender_clone = event_sender.clone();
    let send_task = task::spawn(async move {
        let event_sender = event_sender_clone.clone();
        async move {
            let bytes_sent = payload.len();
            send_stream.write_all(payload.as_bytes()).await?;
            event_sender.send(ConnectEvent::Sent {
                bytes_sent: bytes_sent as u64,
            })
                .await?;
            anyhow::Ok(())
        }
    });
    let n = tokio::io::copy(&mut recv_stream, &mut tokio::io::sink()).await?;
    connection.close(1u8.into(), b"done");
    event_sender.send(ConnectEvent::Received {
        bytes_received: n as u64,
    }).await?;
    send_task.await?.await?;
    Ok(())

}