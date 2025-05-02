use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use das::proxy::PatternMatchingQueryProxy;
use das::translator::translate;
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
    // Parsing possible parameters: ((max_query_answers) (query))
    let (max_query_answers, multi_tokens) = match query {
        Atom::Expression(exp_atom) => {
            let children = exp_atom.children();

            let is_exp = match children.get(0).unwrap() {
                Atom::Symbol(s) => if s.name() == "," { true } else { false },
                Atom::Expression(_) => true,
                _ => return Ok(bindings_set),
            };

            let max_query_answers = 0;

            let mut multi_tokens: Vec<Vec<String>> = vec![];
            if is_exp {
                for atom in children.iter() {
                    if atom.to_string() == "," {
                        continue;
                    }
                    multi_tokens.push(atom.to_string().split_whitespace().map(String::from).collect());
                }
            } else {
                multi_tokens.push(query.to_string().split_whitespace().map(String::from).collect());
            }

            (max_query_answers, multi_tokens)
        }
        _ => return Ok(bindings_set),
    };

    // Translating to LT and setting the VARIABLES
    let mut query = vec![];
    if multi_tokens.len() > 1 {
        query.extend(["AND".to_string(), format!("{}", multi_tokens.len())]);
    }
    let mut variables = HashMap::new();
    for tokens in &multi_tokens {
        for word in tokens {
            if word.starts_with("$") {
                variables.insert(word.replace("$", "").replace(")", ""), "".to_string());
            }
        }
        // Translate MeTTa into LINK_TEMPLATE
        let translation: Vec<String> = translate(&tokens.join(" ")).split_whitespace().map(String::from).collect();
        log::debug!(target: "das", "LT: <{}>", translation.join(" "));
        query.extend(translation);
    }

    log::debug!(target: "das", "Query: <{}>", query.join(" "));

    // Query's params:
    let context = match space_name {
        Some(name) => name.clone(),
        None => "context".to_string(),
    };

    let count_only = false;
    let update_attention_broker = false;
    let unique_assignment = true;

    let mut proxy = PatternMatchingQueryProxy::new(query, context, unique_assignment, update_attention_broker, count_only)?;

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
            for (key, value) in &variables {
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
