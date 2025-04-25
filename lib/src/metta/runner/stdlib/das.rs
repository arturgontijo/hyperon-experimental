use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use das::proxy::PatternMatchingQueryProxy;
use das::types::BoxError;

use das::service_bus::ServiceBus;
use das::service_bus_singleton::ServiceBusSingleton;

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

fn init_service_bus(
    host_id: String,
    known_peer: String,
) -> Result<ServiceBus, BoxError> {
    ServiceBusSingleton::init(host_id, known_peer, 64000, 64999)?;
	Ok(ServiceBusSingleton::get_instance())
}

fn extract_host_id(atom: &Atom) -> Result<String, ExecError> {
    let endpoint = atom.to_string().replace("(", "").replace(")", "");
    if let Some((_, port_str)) = endpoint.split_once(':') {
        if let Ok(_) = port_str.parse::<u16>() {
            return Ok(endpoint);
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
            let host_id = extract_host_id(server)?;
            let known_peer = extract_host_id(client)?;
            let service_bus = Arc::new(Mutex::new(init_service_bus(host_id, known_peer).unwrap()));
            let space = Atom::gnd(DynSpace::new(DistributedAtomSpace::new(
                service_bus,
                Some("context".to_string()),
            )));
            log::debug!(target: "das", "new-das initialized.");
            Ok(vec![space])
        } else {
            Err("new-das expects 2 arguments (eg !(new-das 0.0.0.0:8080 0.0.0.0:35700)".into())
        }
    }
}

pub fn register_context_dependent_tokens(tref: &mut Tokenizer) {
    let new_das_op = Atom::gnd(NewDasOp {});
    tref.register_token(regex(r"new-das"), move |_| new_das_op.clone());
}

pub fn query_with_das(
    space_name: Option<String>,
    service_bus: Arc<Mutex<ServiceBus>>,
    query: &Atom,
) -> Result<BindingsSet, BoxError> {
    let mut bindings_set = BindingsSet::empty();
    // Parsing possible parameters: ((count) (importance) (query))
    let (max_query_answers, tokens) = match query {
        Atom::Expression(_) => {
            let query_inner = query.clone().to_string().replace("(", "").replace(")", "");
            let mut tokens: Vec<String> = query_inner.split_whitespace().map(String::from).collect();
            let mut max_query_answers = 0;
            if tokens.len() > 1 {
                max_query_answers = match tokens[0].parse::<usize>() {
                    Ok(v) => {
                        tokens.remove(0);
                        v
                    },
                    Err(_) => 0,
                }
            }
            (max_query_answers, tokens)
        }
        _ => return Ok(bindings_set),
    };

    // Getting the VARIABLES
    let mut variables = HashMap::new();
    for (idx, word) in tokens.clone().iter().enumerate() {
        if *word == "VARIABLE" {
            variables.insert(tokens[idx + 1].to_string(), "".to_string());
        }
    }

    // Query's params:
    let context = match space_name {
        Some(name) => name.clone(),
        None => "context".to_string(),
    };

    let count_only = false;
    let update_attention_broker = false;
    let unique_assignment = true;

    let mut proxy = PatternMatchingQueryProxy::new(tokens, context, unique_assignment, update_attention_broker, count_only)?;

    let mut service_bus = service_bus.lock().unwrap();
    service_bus.issue_bus_command(&mut proxy)?;

    while !proxy.finished() {
        if let Some(query_answer) = proxy.pop() {
            log::trace!(target: "das", "{}", query_answer.to_string());

            let splitted: Vec<&str> = query_answer.split_whitespace().collect();
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

            if max_query_answers > 0 && bindings_set.len() >= max_query_answers {
                break;
            }

        } else {
            sleep(Duration::from_millis(100));
        }
    }

    log::trace!(target: "das", "BindingsSet: {:?} (len={})", bindings_set, bindings_set.len());

    Ok(bindings_set)
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
