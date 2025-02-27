use std::collections::HashMap;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use std::{thread, u16};

use das::{DASNode, GrpcServer, ServerStatus};
use tokio::sync::Mutex;

use super::{grounded_op, regex};
use crate::matcher::{Bindings, BindingsSet};
use crate::metta::text::Tokenizer;
use crate::metta::*;
use crate::space::distributed::DistributedAtomSpace;
use crate::{space::DynSpace, *};

#[derive(Clone, Debug)]
pub struct NewDasOp {}

grounded_op!(NewDasOp, "new-das");

impl Grounded for NewDasOp {
    fn type_(&self) -> Atom {
        Atom::expr([
            ARROW_SYMBOL,
            rust_type_atom::<DynSpace>(),
            ATOM_TYPE_SYMBOL,
            ATOM_TYPE_SYMBOL,
        ])
    }

    fn as_execute(&self) -> Option<&dyn CustomExecute> {
        Some(self)
    }
}

fn new_das_node(
    server_host: String,
    server_port: u16,
    client_host: String,
    client_port: u16,
) -> Arc<Mutex<DASNode>> {
    let das_node = Arc::new(Mutex::new(DASNode::default()));
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

        *das_node_clone.lock().await = node;
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
    Err(ExecError::from(
        "new-das arguments must be a valid endpoint (eg. 0.0.0.0:8080)",
    ))
}

impl CustomExecute for NewDasOp {
    fn execute(&self, args: &[Atom]) -> Result<Vec<Atom>, ExecError> {
        if args.len() == 2 {
            let server = args.get(0).ok_or(ExecError::from(
                "new-das first argument must be a valid endpoint (eg. 0.0.0.0:8080)",
            ))?;
            let client = args.get(1).ok_or(ExecError::from(
                "new-das second argument must be a valid endpoint (eg. 0.0.0.0:35700)",
            ))?;
            let (server_host, server_port) = extract_host_and_port(server)?;
            let (client_host, client_port) = extract_host_and_port(client)?;
            let das_node = new_das_node(server_host, server_port, client_host, client_port);
            let space = Atom::gnd(DynSpace::new(DistributedAtomSpace::new(
                das_node,
                Some("context".to_string()),
            )));
            Ok(vec![space])
        } else {
            Err("new-das expects 2 arguments (eg !(new-das 0.0.0.0:8080 0.0.0.0:35700)".into())
        }
    }
}

pub fn register_common_tokens(tref: &mut Tokenizer) {
    let new_das_op = Atom::gnd(NewDasOp {});
    tref.register_token(regex(r"new-das"), move |_| new_das_op.clone());
}

pub fn query_with_das(
    space_name: Option<String>,
    das_node: &Arc<Mutex<DASNode>>,
    query: &Atom,
) -> BindingsSet {
    let mut bindings_set = BindingsSet::empty();
    // Parsing possible parameters: ((count) (importance) (query))
    let (count, skip, importance, query_strip) = match query {
        Atom::Expression(exp_atom) => {
            let children = exp_atom.children();
            let exp_len = children.len();

            let is_exp = match children.get(0).unwrap() {
                Atom::Symbol(_) => false,
                Atom::Expression(_) => true,
                _ => return bindings_set,
            };

            let mut query_strip = query.clone().to_string().replace("(", "").replace(")", "");
            let mut count = 0;
            let mut skip = 0;
            let mut importance = 0;
            if is_exp {
                if exp_len == 1 {
                    query_strip = children
                        .get(0)
                        .unwrap()
                        .to_string()
                        .replace("(", "")
                        .replace(")", "");
                } else if exp_len == 2 {
                    let count_skip = children
                        .get(0)
                        .unwrap()
                        .to_string()
                        .replace("(", "")
                        .replace(")", "");
                    let splitted: Vec<_> = count_skip.split(":").collect();
                    count = splitted[0].parse::<usize>().unwrap();
                    if splitted.len() == 2 {
                        skip = splitted[1].parse::<usize>().unwrap();
                    }
                    query_strip = children
                        .get(1)
                        .unwrap()
                        .to_string()
                        .replace("(", "")
                        .replace(")", "");
                } else if exp_len == 3 {
                    let count_skip = children
                        .get(0)
                        .unwrap()
                        .to_string()
                        .replace("(", "")
                        .replace(")", "");
                    let splitted: Vec<_> = count_skip.split(":").collect();
                    count = splitted[0].parse::<usize>().unwrap();
                    if splitted.len() == 2 {
                        skip = splitted[1].parse::<usize>().unwrap();
                    }
                    let importance_str = children
                        .get(1)
                        .unwrap()
                        .to_string()
                        .replace("(", "")
                        .replace(")", "");
                    importance = importance_str.parse::<u32>().unwrap();
                    query_strip = children
                        .get(2)
                        .unwrap()
                        .to_string()
                        .replace("(", "")
                        .replace(")", "");
                }
            }
            (count, skip, importance, query_strip)
        }
        _ => return bindings_set,
    };

    // Getting the VARIABLES
    let mut variables: HashMap<String, String> = HashMap::new();
    let cloned = query_strip.clone();
    let splitted: Vec<&str> = cloned.split_whitespace().collect();
    for (idx, word) in splitted.clone().iter().enumerate() {
        if *word == "VARIABLE" {
            variables.insert(splitted[idx + 1].to_string(), "".to_string());
        }
    }

    // DASNode::query() params:
    let pattern = query_strip.clone();
    let context = match space_name {
        Some(name) => name.clone(),
        None => "context".to_string(),
    };
    let update_attention_broker = false;

    {
        let das_node_clone = Arc::clone(&das_node);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut node = das_node_clone.lock().await;
            if node.is_complete() {
                log::debug!(target: "das", "DASNode::query(params): count:skip=({:?}:{:?}) | importance={:?} | q={:?}", count, skip, importance, query_strip);
                node.query(&pattern, &context, update_attention_broker, count, skip).await.unwrap();
            }
        });
    }

    sleep(Duration::from_millis(250));

    let mut waiting = true;
    while waiting {
        log::trace!(target: "das", "DASNode::while(sleep)...");
        match das_node.try_lock() {
            Ok(node) => {
                let n = node.clone();
                log::trace!(target: "das", "DASNode::while(status): {:?}", n.get_status());

                let results = n.get_results();
                log::trace!(target: "das", "DASNode::while(results): len={:?}", results.len());
                for result in &results {
                    let splitted: Vec<&str> = result.split_whitespace().collect();
                    for (idx, word) in splitted.clone().iter().enumerate() {
                        if let Some(value) = variables.get_mut(&word.to_string()) {
                            *value = splitted[idx + 1].to_string();
                        }
                    }
                    let mut bindings = Bindings::new();
                    for key in variables.keys() {
                        let value = variables.get(key).unwrap();
                        bindings = bindings
                            .add_var_binding(&VariableAtom::new(key), &Atom::sym(value))
                            .unwrap();
                    }
                    bindings_set.push(bindings);
                    if count > 0 && bindings_set.len() >= count {
                        break;
                    }
                }

                if n.get_status() == ServerStatus::Ready || count > 0 && bindings_set.len() >= count
                {
                    waiting = false;
                }
            }
            Err(err) => {
                log::trace!(target: "das", "DASNode::while(locked): {:?}", err);
            }
        }
        sleep(Duration::from_millis(250));
    }

    // ----- NO DAS -----
    // > !(match &self (, (Similarity $1 $2) (Inheritance $2 "plant")) (Similarity $1 $2))
    // BindingsSet[len=2]: BindingsSet([{ $1 <- "snake", $2 <- "vine" }, { $1 <- "human", $2 <- "ent" }])
    // BindingsSet[len=0]: BindingsSet([])
    // [(Similarity "snake" "vine"), (Similarity "human" "ent")]

    // !(include 5000)

    // ----- DAS -----
    // curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    // RUST_LOG=das=debug ./target/release/metta-repl

    // !(bind! &das (new-das (0.0.0.0:8080) (0.0.0.0:35700)))
    // !(match &das (AND 2 LINK_TEMPLATE Expression 3 NODE Symbol Similarity VARIABLE V1 VARIABLE V2 LINK_TEMPLATE Expression 3 NODE Symbol Inheritance VARIABLE V2 NODE Symbol "plant") (Similarity $V1 $V2))
    // BindingsSet[len=2]: BindingsSet([{ $V1 <- 8860480382d0ddf62623abf5c860e51d, $V2 <- a408f6dd446cdd4fa56f82e77fe6c870 }, { $V1 <- 25bdf4cba0b59adfa07dd103d033bca9, $V2 <- 1fc9300891b7a5d6583f0f85a83b9ddb }])
    // [(Similarity 8860480382d0ddf62623abf5c860e51d a408f6dd446cdd4fa56f82e77fe6c870), (Similarity 25bdf4cba0b59adfa07dd103d033bca9 1fc9300891b7a5d6583f0f85a83b9ddb)]

    // !(match &das (LINK_TEMPLATE Expression 3 NODE Symbol Similarity VARIABLE V1 VARIABLE V2) (Similarity $V1 $V2))
    // BindingsSet[len=2]: BindingsSet([{ $V1 <- 8860480382d0ddf62623abf5c860e51d, $V2 <- a408f6dd446cdd4fa56f82e77fe6c870 }, { $V1 <- 25bdf4cba0b59adfa07dd103d033bca9, $V2 <- 1fc9300891b7a5d6583f0f85a83b9ddb }])
    // [(Similarity 8860480382d0ddf62623abf5c860e51d a408f6dd446cdd4fa56f82e77fe6c870), (Similarity 25bdf4cba0b59adfa07dd103d033bca9 1fc9300891b7a5d6583f0f85a83b9ddb)]

    // !(match &das ((1) (LINK_TEMPLATE Expression 3 NODE Symbol Inheritance VARIABLE V2 NODE Symbol "mammal")) (Inheritance $V2))
    // BindingsSet[len=1]: BindingsSet([{ $V2 <- 3225ea795289574ceee32e091ad54ef4 }])
    // [(Inheritance 3225ea795289574ceee32e091ad54ef4)]

    // !(add-atom &das (Similarity 8860480382d0ddf62623abf5c860e51d a408f6dd446cdd4fa56f82e77fe6c870))
    // !(add-atom &das (Similarity 25bdf4cba0b59adfa07dd103d033bca9 1fc9300891b7a5d6583f0f85a83b9ddb))
    // !(get-atoms &das)
    // [(Similarity 8860480382d0ddf62623abf5c860e51d a408f6dd446cdd4fa56f82e77fe6c870), (Similarity 25bdf4cba0b59adfa07dd103d033bca9 1fc9300891b7a5d6583f0f85a83b9ddb)]

    // ----- Python -----
    // concepts = []
    // for i in range(5000):
    //   concepts.append(f"(: {i+1} Concept)\n")

    // similars = []
    // for i in range(5000):
    //   if i >= 5000: break
    //   similars.append(f"(Similarity {i+1} {i+3})\n")

    // with open("5000.metta", "w+") as f:
    //   f.write("(: Similarity Type)\n")
    //   f.write("(: Concept Type)\n")
    //   f.writelines(concepts)
    //   f.writelines(similars)

    // RUST_LOG=das=trace ./target/release/metta-repl
    // !(bind! &das (new-das (0.0.0.0:8080) (0.0.0.0:35700)))
    // !(match &das (LINK_TEMPLATE Expression 3 NODE Symbol Similarity VARIABLE V1 NODE Symbol 4000) (Similarity $V1 4000))

    // Heavy query
    // !(match &das (LINK_TEMPLATE Expression 3 NODE Symbol Similarity VARIABLE V1 VARIABLE V2) (Similarity $V1 $V2))
    // !(match &das ((3) (LINK_TEMPLATE Expression 3 NODE Symbol Similarity VARIABLE V1 VARIABLE V2)) (Similarity $V1 $V2))

    // RUST_LOG=das=debug ./target/release/metta-repl
    // !(bind! &das (new-das (172.18.0.4:8080) (das-query-agent:35700)))

    log::trace!(target: "das", "DASNode::query(das): BindingsSet[len={}]: {:?}", bindings_set.len(), bindings_set);
    bindings_set
}

#[cfg(test)]
mod tests {
    use crate::{
        metta::runner::stdlib::{das::NewDasOp, unit_result},
        sym, CustomExecute,
    };

    #[test]
    fn das_op() {
        assert_eq!(NewDasOp {}.execute(&mut vec![sym!("A")]), unit_result());
    }
}
