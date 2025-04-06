# PostgreSQL Database Dump Utility

A command-line tool written in Rust to create SQL dump files from PostgreSQL databases. The dump includes tables, data, sequences, types, users, roles, and their permissions.

## Installation

1. Make sure you have Rust and Cargo installed. If not, install from [https://rustup.rs/](https://rustup.rs/)
2. Clone this repository
3. Build the project:
   ```
   cargo build --release
   ```
4. The executable will be available at `target/release/database-dump`

## Usage

```
database-dump --host <host> --port <port> --dbname <database> --user <username> --password <password> [--output <filename>]
```

### Options:

- `-h, --host`: Database host (required)
- `-P, --port`: Database port (default: 5432)
- `-d, --dbname`: Database name (required)
- `-u, --user`: Database user (required)
- `-p, --password`: Database password (required)
- `-o, --output`: Output SQL file (optional, default: prints to stdout)

## Example

```
database-dump --host localhost --dbname mydb --user postgres --password mypassword --output database_dump.sql
```

## What Gets Exported

The tool generates a full SQL dump file that includes:

1. **Database Schema**
   - Custom data types (enums)
   - Sequences
   - Tables with column definitions
   - Primary keys, foreign keys, and other constraints
   - Indexes

2. **Table Data**
   - All data from all tables in SQL INSERT statements

3. **Users and Permissions**
   - User/role definitions with attributes (SUPERUSER, LOGIN, etc.)
   - Permissions at the schema level
   - Permissions at the table level
   - Role membership

## Importing the Dump

The generated SQL file can be imported into any PostgreSQL database:

```
psql -U username -d database_name -f database_dump.sql
```

## Security Note

Providing passwords on the command line may expose them in your shell history. 
Consider using environment variables or connection strings in production environments. 