use std::{env, thread::sleep, time::Duration};

use das::{
	proxy::PatternMatchingQueryProxy, service_bus_singleton::ServiceBusSingleton, types::BoxError,
};

const MAX_QUERY_ANSWERS: u32 = 100;

fn main() -> Result<(), BoxError> {
	env_logger::init();

	// ./bin/query localhost:11234 localhost:35700 1 LINK_TEMPLATE ...
	let args: Vec<String> = env::args().collect();

	if args.len() < 5 {
		panic!("Usage: {} CLIENT_HOST:CLIENT_PORT SERVER_HOST:SERVER_PORT UPDATE_ATTENTION_BROKER QUERY_TOKEN+ (hosts are supposed to be public IPs or known hostnames)", &args[0]);
	}

	let client_id = &args[1];
	let server_id = &args[2];
	let update_attention_broker = &args[3] == "true" || &args[3] == "1";
	let unique_assignment = true;

	let context = "";
	let mut query = vec![];

	let mut tokens_start_position = 4;

	let max_query_answers = match (args[4]).parse::<u32>() {
		Ok(value) => {
			tokens_start_position += 1;
			value
		},
		Err(_) => MAX_QUERY_ANSWERS,
	};

	log::info!("Using max_query_answers: {}", max_query_answers);

	for arg in args.iter().skip(tokens_start_position) {
		query.push(arg.clone());
	}

	ServiceBusSingleton::init(client_id.to_string(), server_id.to_string(), 64000, 64999)?;
	let mut service_bus = ServiceBusSingleton::get_instance();

	let mut proxy = PatternMatchingQueryProxy::new(
		query,
		context.to_string(),
		unique_assignment,
		update_attention_broker,
		false,
	)?;

	service_bus.issue_bus_command(&mut proxy)?;

	let mut count = 0;
	while !proxy.finished() {
		if let Some(query_answer) = proxy.pop() {
			log::info!("{}", query_answer);
			count += 1;
			if count == max_query_answers {
				break;
			}
		} else {
			sleep(Duration::from_millis(100));
		}
	}

	if count == 0 {
		log::info!("No match for query");
	}

	Ok(())
}
