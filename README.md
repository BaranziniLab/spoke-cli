# spoke-cli

> A command-line interface for querying the [SPOKE](https://spoke.ucsf.edu) (Scalable Precision medicine Open Knowledge Engine) biomedical knowledge graph via Neo4j/Cypher.

**Author:** Wanjun Gu &nbsp;·&nbsp; [wanjun.gu@ucsf.edu](mailto:wanjun.gu@ucsf.edu)

## Overview

SPOKE is a large-scale biomedical knowledge graph developed at UCSF that integrates data from dozens of public databases — connecting diseases, genes, proteins, compounds, pathways, symptoms, variants, anatomy, and more into a unified graph. `spoke-cli` provides a simple terminal interface to run read-only Cypher queries against SPOKE and export results as JSON or CSV.

## Installation

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024, via `rustup`)
- Network access to the SPOKE Neo4j instance

### Build from source

```bash
git clone https://github.com/BaranziniLab/spoke-cli
cd spoke-cli
cargo build --release
```

The compiled binary will be at `target/release/spoke-cli`. Optionally move it onto your `PATH`:

```bash
cp target/release/spoke-cli /usr/local/bin/spoke-cli
```

## Configuration

Credentials are loaded from a `.env` file in the current working directory:

```env
KNOWLEDGE_GRAPH_URI=bolt://spokedev.cgl.ucsf.edu:7687
KNOWLEDGE_GRAPH_USERNAME=neo4j
KNOWLEDGE_GRAPH_PASSWORD=SPOKEdev
KNOWLEDGE_GRAPH_DATABASE=spoke
```

| Variable                   | Description                    | Example                             |
|----------------------------|--------------------------------|-------------------------------------|
| `KNOWLEDGE_GRAPH_URI`      | Bolt URI to the Neo4j instance | `bolt://spokedev.cgl.ucsf.edu:7687` |
| `KNOWLEDGE_GRAPH_USERNAME` | Neo4j username                 | `neo4j`                             |
| `KNOWLEDGE_GRAPH_PASSWORD` | Neo4j password                 | `SPOKEdev`                          |
| `KNOWLEDGE_GRAPH_DATABASE` | Target database name           | `spoke`                             |

## Commands

### `test-connection`

Verifies connectivity and credentials to the Neo4j instance.

```bash
spoke-cli test-connection
```

```
Connecting to bolt://spokedev.cgl.ucsf.edu:7687 ... OK
  uri      : bolt://spokedev.cgl.ucsf.edu:7687
  database : spoke
  user     : neo4j
```

### `glimpse-knowledge-graph`

Introspects the database schema — node labels, relationship types, and property keys — and returns the result as JSON.

```bash
# Print schema to stdout
spoke-cli glimpse-knowledge-graph

# Save schema to a file
spoke-cli glimpse-knowledge-graph --output schema.json
```

| Output Field                   | Description                                          |
|--------------------------------|------------------------------------------------------|
| `node_labels`                  | All node types in the graph (e.g. `Gene`, `Disease`) |
| `relationship_types`           | All edge types (e.g. `ASSOCIATES_DaG`)               |
| `property_keys`                | All property names used across the graph             |
| `node_type_properties`         | Per-label property schemas with types                |
| `relationship_type_properties` | Per-relationship property schemas                    |

### `query`

Executes a read-only Cypher query. Results are saved to a file by default; use `--stdout` to print instead.

```bash
spoke-cli query '<CYPHER>' [OPTIONS]
```

| Flag              | Description                                           | Default                  |
|-------------------|-------------------------------------------------------|--------------------------|
| `--output <FILE>` | Output file name (extension auto-appended if missing) | `<random-hash>.<format>` |
| `--format <FMT>`  | Output format: `json` or `csv`                        | `json`                   |
| `--stdout`        | Print results to stdout instead of saving to a file   | off                      |

> Write operations (`CREATE`, `MERGE`, `SET`, `DELETE`, `DROP`, etc.) are blocked regardless of credentials.

## Examples

**Check connectivity**
```bash
spoke-cli test-connection
```

**Explore available node types**
```bash
spoke-cli query "CALL db.labels() YIELD label RETURN label ORDER BY label" --stdout
```

**Query disease nodes**
```bash
spoke-cli query "MATCH (d:Disease) RETURN d.name, d.identifier LIMIT 10" --stdout
```

**Multiple sclerosis subnetwork — find the disease node**
```bash
spoke-cli query \
  "MATCH (d:Disease) WHERE d.name =~ '(?i).*multiple sclerosis.*' RETURN d.name, d.identifier" \
  --stdout
```

**Multiple sclerosis subnetwork — all direct neighbors (1-hop)**
```bash
spoke-cli query \
  "MATCH (d:Disease)-[r]-(n)
   WHERE d.name =~ '(?i).*multiple sclerosis.*'
   RETURN d.name AS disease, type(r) AS rel_type, labels(n)[0] AS neighbor_type, n.name AS neighbor
   LIMIT 200" \
  --output ms_subnetwork.json
```

**MS-associated genes**
```bash
spoke-cli query \
  "MATCH (d:Disease)-[r]-(g:Gene)
   WHERE d.name =~ '(?i).*multiple sclerosis.*'
   RETURN d.name AS disease, type(r) AS relationship, g.name AS gene
   LIMIT 100" \
  --format csv --output ms_genes.csv
```

**MS-associated compounds**
```bash
spoke-cli query \
  "MATCH (d:Disease)-[r]-(c:Compound)
   WHERE d.name =~ '(?i).*multiple sclerosis.*'
   RETURN d.name AS disease, type(r) AS relationship, c.name AS compound
   LIMIT 100" \
  --output ms_compounds.json
```

**Gene–protein associations**
```bash
spoke-cli query \
  "MATCH (g:Gene)-[r]-(p:Protein) WHERE g.name = 'BRCA1'
   RETURN g.name AS gene, type(r) AS rel, p.name AS protein" \
  --stdout
```

**Immune pathways**
```bash
spoke-cli query \
  "MATCH (p:Pathway)-[r]-(g:Gene)
   WHERE p.name CONTAINS 'immune'
   RETURN p.name AS pathway, g.name AS gene LIMIT 50" \
  --format csv --output immune_pathways.csv
```

**Save full schema**
```bash
spoke-cli glimpse-knowledge-graph --output spoke_schema.json
```

## SPOKE Node Types

SPOKE integrates data across 42+ node types:

| Category       | Node Types                                                                           |
|----------------|--------------------------------------------------------------------------------------|
| Molecular      | `Gene`, `Protein`, `Compound`, `MiRNA`, `Complex`, `ProteinDomain`, `ProteinFamily`  |
| Disease/Health | `Disease`, `Symptom`, `SideEffect`, `PharmacologicClass`                             |
| Biological     | `BiologicalProcess`, `MolecularFunction`, `CellularComponent`, `Pathway`, `Reaction` |
| Cellular       | `Anatomy`, `CellType`, `AnatomyCellType`, `CellLine`                                 |
| Genomic        | `Variant`, `Chromosome`, `Haplotype`, `PanGene`                                      |
| Dietary        | `Food`, `Nutrient`, `DietarySupplement`                                              |
| Other          | `Organism`, `EC`, `Location`, `SDoH`, `Environment`                                  |

## Output Formats

**JSON** — array of objects, one per row, keyed by `RETURN` column names or aliases:

```json
[
  { "disease": "multiple sclerosis", "rel_type": "ASSOCIATES_DaG", "neighbor_type": "Gene",     "neighbor": "HLA-DRB1"          },
  { "disease": "multiple sclerosis", "rel_type": "TREATS_CtD",     "neighbor_type": "Compound", "neighbor": "interferon beta-1a" }
]
```

**CSV** — standard comma-separated with a header row:

```
disease,rel_type,neighbor_type,neighbor
"multiple sclerosis","ASSOCIATES_DaG","Gene","HLA-DRB1"
"multiple sclerosis","TREATS_CtD","Compound","interferon beta-1a"
```

## Dependencies

| Crate        | Purpose                             |
|--------------|-------------------------------------|
| `neo4rs`     | Async Neo4j Bolt driver             |
| `tokio`      | Async runtime                       |
| `clap`       | CLI argument parsing                |
| `dotenvy`    | `.env` file loading                 |
| `serde_json` | JSON serialization                  |
| `regex`      | Cypher write-guard & column parsing |
| `rand`       | Default output filename generation  |

## License

For research and educational use at UCSF. See the [SPOKE project](https://spoke.ucsf.edu) for data licensing terms.
