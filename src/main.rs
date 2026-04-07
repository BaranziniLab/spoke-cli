use clap::{Parser, Subcommand};
use neo4rs::{ConfigBuilder, Graph};
use regex::Regex;
use serde_json::{json, Value};
use std::env;
use std::fs;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "spoke-cli",
    version = "0.1.0",
    about = "SPOKE CLI — Query the SPOKE biomedical knowledge graph (Neo4j/Cypher)",
    long_about = "
SPOKE CLI — SPOKE Biomedical Knowledge Graph Query Tool
═══════════════════════════════════════════════════════

Query the SPOKE (Scalable Precision medicine Open Knowledge Engine) biomedical
knowledge graph using Cypher. Connection credentials are loaded automatically
from a .env file in the current directory.

ENVIRONMENT VARIABLES (via .env):
  KNOWLEDGE_GRAPH_URI        Bolt URI         e.g. bolt://host:7687
  KNOWLEDGE_GRAPH_USERNAME   Neo4j username
  KNOWLEDGE_GRAPH_PASSWORD   Neo4j password
  KNOWLEDGE_GRAPH_DATABASE   Target database  e.g. spoke

COMMANDS:
  test-connection          Verify connectivity and credentials
  glimpse-knowledge-graph  Show schema: node types, relationship types, properties
  query <CYPHER>           Run a read-only Cypher query

EXAMPLES:
  spoke-cli test-connection
  spoke-cli glimpse-knowledge-graph
  spoke-cli glimpse-knowledge-graph --output schema.json

  spoke-cli query 'MATCH (n:Gene) RETURN n.name LIMIT 5'
  spoke-cli query 'MATCH (d:Disease) RETURN d.name LIMIT 10' --format csv
  spoke-cli query 'MATCH (n:Protein) RETURN n.name LIMIT 5' --output proteins.json
  spoke-cli query 'MATCH (n:Compound) RETURN n.name LIMIT 5' --stdout

NOTE:
  Write operations (CREATE, MERGE, SET, DELETE, DROP, etc.) are blocked.
"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Verify connectivity and credentials to the Neo4j knowledge graph
    #[command(name = "test-connection")]
    TestConnection,

    /// Show the knowledge graph schema: node types, relationship types, and property keys.
    /// Output goes to stdout by default.
    #[command(name = "glimpse-knowledge-graph")]
    GlimpseKnowledgeGraph {
        /// Save output to a file instead of printing to stdout
        #[arg(short, long, value_name = "FILE")]
        output: Option<String>,
    },

    /// Execute a read-only Cypher query against the knowledge graph.
    /// Results are saved to a JSON file by default (use --stdout to print instead).
    #[command(name = "query")]
    Query {
        /// Cypher query to execute  e.g. 'MATCH (n:Gene) RETURN n.name LIMIT 10'
        cypher: String,

        /// Output file name. Defaults to <8-char-hash>.<format>
        #[arg(short, long, value_name = "FILE")]
        output: Option<String>,

        /// Output format: json (default) or csv
        #[arg(short, long, default_value = "json", value_parser = ["json", "csv"])]
        format: String,

        /// Print results to stdout instead of saving to a file
        #[arg(short, long)]
        stdout: bool,
    },
}

// ── Environment & connection ──────────────────────────────────────────────────

fn load_env() -> (String, String, String, String) {
    dotenvy::dotenv().ok();
    let uri  = env::var("KNOWLEDGE_GRAPH_URI")
        .expect("KNOWLEDGE_GRAPH_URI not set — add it to your .env file");
    let user = env::var("KNOWLEDGE_GRAPH_USERNAME")
        .expect("KNOWLEDGE_GRAPH_USERNAME not set — add it to your .env file");
    let pass = env::var("KNOWLEDGE_GRAPH_PASSWORD")
        .expect("KNOWLEDGE_GRAPH_PASSWORD not set — add it to your .env file");
    let db   = env::var("KNOWLEDGE_GRAPH_DATABASE")
        .expect("KNOWLEDGE_GRAPH_DATABASE not set — add it to your .env file");
    (uri, user, pass, db)
}

async fn connect(uri: &str, user: &str, pass: &str, db: &str) -> Graph {
    let config = ConfigBuilder::default()
        .uri(uri)
        .user(user)
        .password(pass)
        .db(db)
        .build()
        .unwrap_or_else(|e| { eprintln!("Config error: {e}"); std::process::exit(1) });
    Graph::connect(config).await.unwrap_or_else(|e| {
        eprintln!("Connection failed: {e}");
        std::process::exit(1)
    })
}

// ── Write-query guard ─────────────────────────────────────────────────────────

fn is_write_query(query: &str) -> bool {
    Regex::new(
        r"(?i)\b(MERGE|CREATE|SET|DELETE|REMOVE|DROP|ALTER|TRUNCATE|GRANT|REVOKE|EXEC|EXECUTE|ADD|INSERT|UPDATE)\b"
    )
    .unwrap()
    .is_match(query)
}

// ── Row → JSON ────────────────────────────────────────────────────────────────

/// Best-effort scalar extraction. Tries common Neo4j types in priority order.
/// Complex types (nodes, relationships) fall back to null — use explicit
/// property access in your Cypher (e.g. n.name) for structured output.
fn extract_value(row: &neo4rs::Row, key: &str) -> Value {
    if let Ok(v) = row.get::<bool>(key)         { return json!(v); }
    if let Ok(v) = row.get::<i64>(key)           { return json!(v); }
    if let Ok(v) = row.get::<f64>(key)           { return json!(v); }
    if let Ok(v) = row.get::<String>(key)        { return json!(v); }
    if let Ok(v) = row.get::<Vec<String>>(key)   { return json!(v); }
    if let Ok(v) = row.get::<Vec<i64>>(key)      { return json!(v); }
    if let Ok(v) = row.get::<Vec<f64>>(key)      { return json!(v); }
    Value::Null
}

fn row_to_json(row: &neo4rs::Row, keys: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    for key in keys {
        map.insert(key.clone(), extract_value(row, key));
    }
    Value::Object(map)
}

/// Extract return column names from a Cypher query.
/// Handles AS aliases, dotted properties, YIELD, ORDER BY, LIMIT etc.
fn parse_return_keys(cypher: &str) -> Vec<String> {
    // Match the last RETURN clause, stopping before LIMIT / ORDER BY / SKIP / UNION
    let re = Regex::new(
        r"(?i)\bRETURN\s+(?:DISTINCT\s+)?([\s\S]+?)(?:\s+\b(?:LIMIT|ORDER\s+BY|SKIP|UNION)\b|$)"
    ).unwrap();

    let cols_str = match re.captures(cypher) {
        Some(cap) => cap[1].trim().to_string(),
        None      => return vec![],
    };

    let alias_re = Regex::new(r"(?i)\bAS\s+(\w+)\s*$").unwrap();

    cols_str.split(',').filter_map(|col| {
        let col = col.trim();
        if col.is_empty() { return None; }
        if let Some(a) = alias_re.captures(col) {
            // RETURN n.name AS gene_name  →  "gene_name"
            Some(a[1].to_string())
        } else {
            // Keep the full expression: "n.name", "label", "COUNT(n)", etc.
            // Strip only trailing whitespace/comments — take up to first whitespace
            Some(col.split_whitespace().next().unwrap_or(col).to_string())
        }
    }).collect()
}

// ── CSV serialiser ────────────────────────────────────────────────────────────

fn records_to_csv(records: &[Value]) -> String {
    if records.is_empty() {
        return String::new();
    }
    let headers: Vec<String> = match &records[0] {
        Value::Object(m) => m.keys().cloned().collect(),
        _ => return String::new(),
    };

    let mut out = headers.join(",") + "\n";
    for rec in records {
        if let Value::Object(m) = rec {
            let row: Vec<String> = headers.iter().map(|h| {
                match m.get(h) {
                    Some(Value::String(s)) => format!("\"{}\"", s.replace('"', "\"\"")),
                    Some(Value::Null) | None => String::new(),
                    Some(v) => format!("\"{}\"", v.to_string().replace('"', "\"\"")),
                }
            }).collect();
            out += &(row.join(",") + "\n");
        }
    }
    out
}

// ── Query runner ──────────────────────────────────────────────────────────────

async fn run_query(graph: &Graph, cypher: &str) -> Vec<Value> {
    run_query_optional(graph, cypher).await
        .unwrap_or_else(|e| { eprintln!("Query error: {e}"); std::process::exit(1) })
}

/// Like run_query but returns Err instead of exiting on failure.
async fn run_query_optional(graph: &Graph, cypher: &str) -> Result<Vec<Value>, String> {
    let keys = parse_return_keys(cypher);

    let mut stream = graph
        .execute(neo4rs::query(cypher))
        .await
        .map_err(|e| e.to_string())?;

    let mut records = Vec::new();
    loop {
        match stream.next().await {
            Ok(Some(row)) => records.push(row_to_json(&row, &keys)),
            Ok(None)      => break,
            Err(e)        => return Err(e.to_string()),
        }
    }
    Ok(records)
}

// ── Output helper ─────────────────────────────────────────────────────────────

fn save_file(content: &str, path: &str) {
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("Failed to write {path}: {e}");
        std::process::exit(1);
    });
    println!("Saved → {path}");
}

fn random_hash() -> String {
    format!("{:08x}", rand::random::<u32>())
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (uri, user, pass, db) = load_env();

    match cli.command {

        // ── test-connection ──────────────────────────────────────────────────
        Commands::TestConnection => {
            print!("Connecting to {} ... ", uri);
            let graph = connect(&uri, &user, &pass, &db).await;
            let mut result = graph
                .execute(neo4rs::query("RETURN 'OK' AS status"))
                .await
                .expect("Test query failed");

            if let Ok(Some(row)) = result.next().await {
                let status: String = row.get("status").unwrap_or_default();
                println!("{}", status);
                println!("  uri      : {uri}");
                println!("  database : {db}");
                println!("  user     : {user}");
            }
        }

        // ── glimpse-knowledge-graph ──────────────────────────────────────────
        Commands::GlimpseKnowledgeGraph { output } => {
            let graph = connect(&uri, &user, &pass, &db).await;

            eprintln!("Fetching knowledge graph schema...");

            macro_rules! try_query {
                ($graph:expr, $cypher:expr, $label:expr) => {{
                    match run_query_optional($graph, $cypher).await {
                        Ok(rows) => rows,
                        Err(e) => {
                            eprintln!("  [warn] {}: {}", $label, e);
                            vec![]
                        }
                    }
                }};
            }

            let node_labels = try_query!(
                &graph,
                "CALL db.labels() YIELD label RETURN label ORDER BY label",
                "node labels"
            );

            let rel_types = try_query!(
                &graph,
                "CALL db.relationshipTypes() YIELD relationshipType \
                 RETURN relationshipType ORDER BY relationshipType",
                "relationship types"
            );

            let prop_keys = try_query!(
                &graph,
                "CALL db.propertyKeys() YIELD propertyKey \
                 RETURN propertyKey ORDER BY propertyKey",
                "property keys"
            );

            let node_schema = try_query!(
                &graph,
                "CALL db.schema.nodeTypeProperties() \
                 YIELD nodeType, nodeLabels, propertyName, propertyTypes, mandatory \
                 RETURN nodeType, nodeLabels, propertyName, propertyTypes, mandatory \
                 ORDER BY nodeType, propertyName",
                "node type properties"
            );

            let rel_schema = try_query!(
                &graph,
                "CALL db.schema.relTypeProperties() \
                 YIELD relType, propertyName, propertyTypes, mandatory \
                 RETURN relType, propertyName, propertyTypes, mandatory \
                 ORDER BY relType, propertyName",
                "relationship type properties"
            );

            let schema = json!({
                "node_labels": node_labels.iter()
                    .filter_map(|v| v.get("label").cloned())
                    .collect::<Vec<_>>(),
                "relationship_types": rel_types.iter()
                    .filter_map(|v| v.get("relationshipType").cloned())
                    .collect::<Vec<_>>(),
                "property_keys": prop_keys.iter()
                    .filter_map(|v| v.get("propertyKey").cloned())
                    .collect::<Vec<_>>(),
                "node_type_properties": node_schema,
                "relationship_type_properties": rel_schema,
            });

            let text = serde_json::to_string_pretty(&schema).unwrap();

            match output {
                Some(path) => save_file(&text, &path),
                None       => println!("{text}"),
            }
        }

        // ── query ────────────────────────────────────────────────────────────
        Commands::Query { cypher, output, format, stdout } => {
            if is_write_query(&cypher) {
                eprintln!(
                    "Error: write operations (CREATE, MERGE, SET, DELETE, DROP, etc.) \
                     are not permitted on this knowledge graph."
                );
                std::process::exit(1);
            }

            let graph = connect(&uri, &user, &pass, &db).await;
            let records = run_query(&graph, &cypher).await;

            let (content, ext) = match format.as_str() {
                "csv" => (records_to_csv(&records), "csv"),
                _     => (serde_json::to_string_pretty(&records).unwrap(), "json"),
            };

            if stdout {
                println!("{content}");
            } else {
                let filename = match output {
                    Some(name) => {
                        if name.ends_with(&format!(".{ext}")) {
                            name
                        } else {
                            format!("{name}.{ext}")
                        }
                    }
                    None => format!("{}.{ext}", random_hash()),
                };
                save_file(&content, &filename);
            }
        }
    }
}
