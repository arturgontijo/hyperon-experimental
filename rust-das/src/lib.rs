use tonic::{
    transport::Server,
    Request,
    Response,
    Status
};

use std::sync::Arc;
use tokio::sync::Mutex;

use das_proto::atom_space_node_server::{AtomSpaceNode, AtomSpaceNodeServer};
use das_proto::atom_space_node_client::AtomSpaceNodeClient;
use das_proto::{MessageData, Ack, Empty};

mod das_proto {
    tonic::include_proto!("dasproto");
}

#[derive(Default, Clone, Debug, PartialEq)]
pub enum ServerStatus {
    #[default]
    Ready,
    Processing,
    Stopped,
    Unknown,
}

#[derive(Default, Clone, Debug)]
pub struct DASNodeStatus(ServerStatus);

impl DASNodeStatus {
    fn change_status(&mut self, status: ServerStatus) {
        self.0 = status;
    }
}

#[derive(Default, Clone, Debug)]
pub struct DASNode {
    server_host: String,
    server_port: u16,
    client_host: String,
    client_port: u16,
    pub status: Arc<Mutex<DASNodeStatus>>,
    pub results: Arc<Mutex<Vec<String>>>,
}

impl DASNode {
    pub async fn new(
        server_host: String,
        server_port: u16,
        client_host: String,
        client_port: u16
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(DASNode {
            server_host,
            server_port,
            client_host,
            client_port,
            status: Arc::new(Mutex::new(DASNodeStatus::default())),
            results: Arc::new(Mutex::new(vec![])),
        })
    }

    async fn send(&self, request: Request<MessageData>) -> Result<Response<Empty>, Status> {
        let target_addr = format!("http://{}:{}", self.client_host, self.client_port);
        match AtomSpaceNodeClient::connect(target_addr).await {
            Ok(mut client) => return Ok(client.execute_message(request).await?),
            Err(err) => {
                println!("DASNode::send(ERROR): {:?}", err);
                return Err(Status::internal("Client failed to connect with remote!"));
            },
        };
    }

    pub async fn query(&mut self, pattern: &str, context: &str, update_attention_broker: bool) -> Result<Response<Empty>, Status> {
        self.set_status(ServerStatus::Processing).await;

        let mut args = vec![
            format!("{}:{}", self.server_host, self.server_port),
            context.to_string(),
            update_attention_broker.to_string()
        ];
        let pattern = pattern.split_whitespace().map(|token| token.to_string()).collect::<Vec<String>>();
        args.extend(pattern);

        let request = Request::new(MessageData {
            command: "pattern_matching_query".to_string(),
            args,
            sender: format!("{}:{}", self.server_host, self.server_port),
            is_broadcast: false,
            visited_recipients: vec![],
        });

        self.send(request).await
    }

    pub async fn get_results_async(&self) -> Vec<String> {
        let mut results_lock = self.results.lock().await;
        let results = std::mem::take(&mut *results_lock);
        results
    }

    pub fn get_results(&self) -> Vec<String> {
        match self.results.try_lock() {
            Ok(mut r) => std::mem::take(&mut *r),
            Err(_) => vec![],
        }
    }

    pub fn get_status(&self) -> ServerStatus {
        match self.status.try_lock() {
            Ok(s) => s.0.clone(),
            Err(_) => ServerStatus::Unknown,
        }
    }

    async fn set_status(&mut self, status: ServerStatus) {
        let mut s = self.status.lock().await;
        s.change_status(status);
    }

    pub async fn stop(&mut self) {
        self.set_status(ServerStatus::Stopped).await;
    }

    pub fn is_complete(&self) -> bool {
        if let Some(status) = self.status.try_lock().ok() {
            status.0 != ServerStatus::Processing
        } else {
            false
        }
    }

    fn process_message(&self, msg: MessageData) -> (ServerStatus, Vec<String>) {
        log::debug!("DASNode::process_message()[{}:{}]: MessageData -> len={:?}", self.server_host, self.server_port, msg.args.len());
        log::trace!(" -> len={:?}", msg);
        match msg.command.as_str() {
            "node_joined_network" => (ServerStatus::Processing, vec![]),
            "query_answer_tokens_flow" => (ServerStatus::Processing, msg.args),
            "query_answer_flow" => (ServerStatus::Processing, vec![]),
            "pattern_matching_query" => (ServerStatus::Processing, vec![]),
            "query_answers_finished" => (ServerStatus::Ready, vec![]),
            _ => (ServerStatus::Unknown, vec![]),
        }
    }
}

#[tonic::async_trait]
impl AtomSpaceNode for DASNode {
    async fn execute_message(
        &self,
        request: Request<MessageData>,
    ) -> Result<Response<Empty>, Status> {
        let (status, results) = self.process_message(request.into_inner());
        let mut r = self.results.lock().await;
        r.extend(results);
        let mut s = self.status.lock().await;
        s.change_status(status);
        Ok(Response::new(Empty {}))
    }

    async fn ping(&self, _request: Request<Empty>) -> Result<Response<Ack>, Status> {
        Ok(Response::new(Ack { error: false, msg: "ack".into() }))
    }
}

#[tonic::async_trait]
pub trait GrpcServer {
    async fn start_server(self) -> Result<(), Box<dyn std::error::Error>>;
}

#[tonic::async_trait]
impl GrpcServer for DASNode {
    async fn start_server(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("{}:{}", self.server_host, self.server_port).parse()?;
        log::debug!("DASNode::start_server(): Inside gRPC server thread at {:?}", addr);
        Server::builder()
            .add_service(AtomSpaceNodeServer::new(self))
            .serve(addr)
            .await
            .unwrap();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{thread::{self, sleep}, time::Duration};

    use super::*;

    #[tokio::test]
    async fn it_works() -> Result<(), Box<dyn std::error::Error>> {
        // GRPC Server Mock
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let server_host = "0.0.0.0".to_string();
                let server_port = 7777;
                let mock_server = DASNode::new(server_host.clone(), server_port, "0.0.0.0".to_string(), 7778).await.unwrap();
                Server::builder()
                    .add_service(AtomSpaceNodeServer::new(mock_server))
                    .serve(format!("{}:{}", server_host, server_port).parse().unwrap())
                    .await
                    .unwrap();
            });
        });

        let server_host = "0.0.0.0".to_string();
        let server_port = 8080;
        let client_host = "0.0.0.0".to_string();
        let client_port = 7777;

        let mut das_node = DASNode::new(server_host, server_port, client_host, client_port).await?;

        let node_clone = das_node.clone();
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                node_clone.start_server().await.unwrap();
            });
        });

        // Wait for mock server be up and running
        sleep(Duration::from_millis(250));

        match das_node.query("TEST", "context", false).await {
            Ok(_) => println!("OK!"),
            Err(e) => println!("Fail: {:?}", e),
        };

        Ok(())
    }
}
