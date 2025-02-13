use std::sync::Arc;
use std::{thread, u16};

use das::{DASNode, GrpcServer};
use tokio::sync::Mutex;

use crate::{space::DynSpace, *};
use crate::metta::*;
use crate::metta::text::Tokenizer;
use super::{grounded_op, regex};

#[derive(Clone, Debug)]
pub struct NewDasOp {}

grounded_op!(NewDasOp, "new-das");

impl Grounded for NewDasOp {
    fn type_(&self) -> Atom {
        Atom::expr([ARROW_SYMBOL, rust_type_atom::<DynSpace>(), ATOM_TYPE_SYMBOL, ATOM_TYPE_SYMBOL])
    }

    fn as_execute(&self) -> Option<&dyn CustomExecute> {
        Some(self)
    }
}

fn new_das_node(server_host: String, server_port: u16, client_host: String, client_port: u16) -> Arc<Mutex<Option<DASNode>>> {
    let das_node = Arc::new(Mutex::new(None));
    let das_node_clone = Arc::clone(&das_node);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let node = DASNode::new(server_host, server_port, client_host, client_port)
            .await
            .unwrap();

        // Start gRPC server (runs indefinitely)
        {
            let node_clone = node.clone();
            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    node_clone.start_server().await.unwrap();
                });
            });
        }

        *das_node_clone.lock().await = Some(node);
        log::trace!(target: "das", "das::new_das_node(): startup done!");
    });
    das_node
}

fn extract_host_and_port(atom: &Atom) -> Result<(String, u16), ExecError> {
    let endpoint = atom.to_string().replace("(", "").replace(")", "");
    if let Some((host, port_str)) = endpoint.split_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            return Ok((host.to_string(), port));
        }
    }
    Err(ExecError::from("new-das arguments must be a valid endpoint (eg. 0.0.0.0:8080)"))
}

impl CustomExecute for NewDasOp {
    fn execute(&self, args: &[Atom]) -> Result<Vec<Atom>, ExecError> {
        if args.len() == 2 {
            let server = args.get(0).ok_or(ExecError::from("new-das first argument must be a valid endpoint (eg. 0.0.0.0:8080)"))?;
            let client = args.get(1).ok_or(ExecError::from("new-das second argument must be a valid endpoint (eg. 0.0.0.0:35700)"))?;
            let (server_host, server_port) = extract_host_and_port(server)?;
            let (client_host, client_port) = extract_host_and_port(client)?;
            let das_node = new_das_node(server_host, server_port, client_host, client_port);
            let space = Atom::gnd(DynSpace::new(GroundingSpace::new_with_das(das_node)));
            Ok(vec![space])
        } else {
            Err("new-das expects 2 arguments (eg !(new-das 0.0.0.0:8080 0.0.0.0:35700)".into())
        }
    }
}

pub fn register_common_tokens(tref: &mut Tokenizer) {
    let new_das_op = Atom::gnd(NewDasOp{});
    tref.register_token(regex(r"new-das"), move |_| { new_das_op.clone() });
}

#[cfg(test)]
mod tests {
    use crate::{
        metta::runner::stdlib::{das::NewDasOp, unit_result},
        sym,
        CustomExecute
    };

    #[test]
    fn das_op() {
        assert_eq!(NewDasOp{}.execute(&mut vec![sym!("A")]), unit_result());
    }
}
