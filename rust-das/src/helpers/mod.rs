use std::collections::VecDeque;

// Enum to represent the parsed S-expression tokens
#[derive(Debug, Clone)]
enum Token {
	Symbol(String),
	Variable(String),
	OpenParen,
	CloseParen,
}

// Enum to represent the AST nodes
#[derive(Debug)]
enum Node {
	Symbol(String),
	Variable(String),
	Expression(Vec<Node>),
}

// Struct to hold the parser state
struct Parser {
	tokens: VecDeque<Token>,
}

impl Parser {
	fn new(input: &str) -> Self {
		let tokens = tokenize(input);
		Parser { tokens: VecDeque::from(tokens) }
	}

	fn parse(&mut self) -> Option<Node> {
		self.parse_expression()
	}

	fn parse_expression(&mut self) -> Option<Node> {
		if let Some(token) = self.tokens.pop_front() {
			match token {
				Token::OpenParen => {
					let mut nodes = Vec::new();
					while let Some(next_token) = self.tokens.front() {
						match next_token {
							Token::CloseParen => {
								self.tokens.pop_front(); // Consume closing parenthesis
								return Some(Node::Expression(nodes));
							},
							_ => {
								if let Some(node) = self.parse_expression() {
									nodes.push(node);
								} else {
									return None;
								}
							},
						}
					}
					None // Unmatched parenthesis
				},
				Token::Symbol(s) => Some(Node::Symbol(s)),
				Token::Variable(v) => Some(Node::Variable(v)),
				Token::CloseParen => None, // Unexpected closing parenthesis
			}
		} else {
			None
		}
	}
}

// Tokenize the input string into a vector of tokens
fn tokenize(input: &str) -> Vec<Token> {
	let mut tokens = Vec::new();
	let mut chars = input.chars().peekable();
	let mut current = String::new();
	let mut in_quotes = false;

	while let Some(c) = chars.next() {
		match c {
			'"' if !in_quotes => {
				if !current.is_empty() {
					tokens.push(classify_token(&current));
					current.clear();
				}
				in_quotes = true;
				current.push(c);
			},
			'"' if in_quotes => {
				current.push(c);
				tokens.push(Token::Symbol(current.clone()));
				current.clear();
				in_quotes = false;
			},
			'(' if !in_quotes => {
				if !current.is_empty() {
					tokens.push(classify_token(&current));
					current.clear();
				}
				tokens.push(Token::OpenParen);
			},
			')' if !in_quotes => {
				if !current.is_empty() {
					tokens.push(classify_token(&current));
					current.clear();
				}
				tokens.push(Token::CloseParen);
			},
			' ' | '\n' | '\t' if !in_quotes => {
				if !current.is_empty() {
					tokens.push(classify_token(&current));
					current.clear();
				}
			},
			_ => {
				current.push(c);
			},
		}
	}

	if !current.is_empty() {
		tokens.push(classify_token(&current));
	}

	tokens
}

// Classify a string as a Symbol or Variable
fn classify_token(s: &str) -> Token {
	if s.starts_with('$') {
		Token::Variable(s[1..].to_string())
	} else {
		Token::Symbol(s.to_string())
	}
}

// Generate the output string from the AST as a single line
fn generate_output(node: &Node) -> String {
	match node {
		Node::Symbol(s) => format!("NODE Symbol {}", s),
		Node::Variable(v) => format!("VARIABLE {}", v),
		Node::Expression(nodes) => {
			let count = nodes.len();
			let link_type = if needs_link_template(nodes) { "LINK_TEMPLATE" } else { "LINK" };
			let mut parts = vec![format!("{} Expression {}", link_type, count)];
			for node in nodes {
				parts.push(generate_output(node));
			}
			parts.join(" ")
		},
	}
}

// Determine if an expression needs LINK_TEMPLATE
fn needs_link_template(nodes: &[Node]) -> bool {
	for node in nodes {
		match node {
			Node::Variable(_) => return true,
			Node::Expression(sub_nodes) => {
				// Recursively check if the sub-expression needs LINK_TEMPLATE
				if needs_link_template(sub_nodes) {
					return true;
				}
			},
			Node::Symbol(_) => continue,
		}
	}
	false
}

pub fn translate(input: &str) -> String {
	let mut parser = Parser::new(input);
	if let Some(ast) = parser.parse() {
		generate_output(&ast)
	} else {
		"Parse error".to_string()
	}
}

pub fn split_ignore_quoted(s: &str) -> Vec<String> {
	let mut result = Vec::new();
	let mut chars = s.chars().peekable();
	let mut current = String::new();
	let mut in_single_quotes = false;
	let mut in_double_quotes = false;

	while let Some(c) = chars.next() {
		match c {
			'\'' if !in_double_quotes && !in_single_quotes => {
				in_single_quotes = true;
				current.push(c);
			},
			'\'' if !in_double_quotes && in_single_quotes => {
				in_single_quotes = false;
				current.push(c);
				result.push(current.clone());
				current.clear();
			},
			'"' if !in_single_quotes && !in_double_quotes => {
				in_double_quotes = true;
				current.push(c);
			},
			'"' if !in_single_quotes && in_double_quotes => {
				in_double_quotes = false;
				current.push(c);
				result.push(current.clone());
				current.clear();
			},
			c if (c == ' ' || c == '\t' || c == '\n') && !in_single_quotes && !in_double_quotes => {
				if !current.is_empty() {
					result.push(current.clone());
					current.clear();
				}
			},
			_ => {
				current.push(c);
			},
		}
	}

	if !current.is_empty() {
		result.push(current);
	}

	result
}
