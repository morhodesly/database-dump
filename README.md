# PostgreSQL Database Dump Utility

A robust command-line tool written in Rust for creating comprehensive SQL dump files from PostgreSQL databases. This utility specializes in exporting complete database structures including tables, data, sequences, types, users, roles, and their permissions - all properly ordered for seamless reimporting.

Unlike standard pg_dump, this tool focuses on creating clean, selective dumps with proper role permissions and database associations. Perfect for database migrations, backups, and development environment setup.

![GitHub](https://img.shields.io/github/license/morhodesly/database-dump)

## Features

- Creates complete PostgreSQL database dumps with proper dependency ordering
- Exports only users/roles directly associated with the database 
- Includes tables, data, sequences, types, constraints, and indexes
- Creates database dump files in a dedicated directory
- Simple command-line interface with sensible defaults
- Cross-platform compatibility (Linux, macOS, Windows)
- Resolves common pg_dump issues with role permissions and type handling

## Comparison with pg_dump

While PostgreSQL's built-in pg_dump tool is powerful, this utility addresses several limitations:

| Feature | PostgreSQL Database Dump Utility | Standard pg_dump |
|---------|----------------------------------|------------------|
| Role Export | ✅ Only exports roles associated with the database | ❌ Requires separate handling with pg_dumpall |
| Role Permissions | ✅ Preserves exact role permissions | ❌ Often requires manual adjustment |
| Dependency Handling | ✅ Optimized dependency resolution | ✅ Good but sometimes requires manual ordering |
| Custom Types | ✅ Full support with proper ordering | ⚠️ Sometimes requires special handling |
| Output Organization | ✅ Structured, categorized output | ⚠️ Requires custom options to organize |
| Ease of Use | ✅ Simple command options | ⚠️ Requires multiple options for complete dumps |
| Default Output | ✅ Creates files by default | ⚠️ Defaults to stdout |

This tool is particularly useful when you need to create deployable SQL dumps that include proper role handling, which is typically challenging with standard tools.

## Installation

### Pre-built Binaries

You can download pre-built binaries for Windows, macOS, and Linux from the [Releases](https://github.com/morhodesly/database-dump/releases) page.

### Building from Source

1. Make sure you have Rust and Cargo installed. If not, install from [https://rustup.rs/](https://rustup.rs/)
2. Clone this repository
   ```
   git clone https://github.com/morhodesly/database-dump.git
   cd database-dump
   ```
3. Build the project:
   ```
   cargo build --release
   ```
4. The executable will be available at `target/release/database-dump`

## Usage

```
database-dump --host <host> [--port <port>] --dbname <database> --user <username> --password <password> [--output <filename>]
```

### Options:

- `-h, --host`: Database host (required)
- `-P, --port`: Database port (default: 5432)
- `-d, --dbname`: Database name (required)
- `-u, --user`: Database user (required)
- `-p, --password`: Database password (required)
- `-o, --output`: Output SQL file (optional, default: `<dbname>-dump.sql`)

## Example

```
# With explicit output filename
database-dump --host localhost --dbname mydb --user postgres --password mypassword --output custom_name.sql

# Without output filename (uses default)
database-dump --host localhost --dbname mydb --user postgres --password mypassword
```

## Output Location

All dump files are saved to the `dump-output` directory in the current working directory. This directory is automatically created if it doesn't exist. 

Default file naming: If no output filename is specified, the tool automatically uses `<dbname>-dump.sql` as the filename (e.g., `mydb-dump.sql`).

## What Gets Exported

The tool generates a full SQL dump file that includes:

1. **Users and Roles** (first in the file, for proper import order)
   - Only roles directly associated with the database (database owner and object owners)
   - User/role definitions with attributes (SUPERUSER, LOGIN, etc.)
   - Role membership relationships

2. **Database Schema**
   - Custom data types (enums)
   - Sequences
   - Tables with column definitions
   - Primary keys, foreign keys, and other constraints
   - Indexes

3. **Table Data**
   - All data from all tables as SQL INSERT statements

## Importing the Dump

The generated SQL file can be imported into any PostgreSQL database:

```
psql -U username -d database_name -f dump-output/mydb-dump.sql
```

## Security Note

Providing passwords on the command line may expose them in your shell history. 
Consider using environment variables or connection strings in production environments.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

Please make sure your code follows the existing style and includes appropriate tests.

## Creating a Release

This project uses a single GitHub Actions workflow to automatically create releases when you update the version in `Cargo.toml`:

1. Update the version in `Cargo.toml` (e.g., change `version = "0.1.3"` to `version = "0.1.4"`)
2. Commit your changes: `git commit -m "Bump version to x.y.z"`
3. Push to the master branch: `git push origin master`

The workflow will automatically:
- Detect the version change in Cargo.toml
- Create a new tag based on that version (e.g., v0.1.4)
- Create a GitHub release
- Build binaries for Windows, macOS, and Linux
- Upload the binaries to the release

You don't need to manually create tags or releases - just update the version in Cargo.toml and push to master!

## Issues

If you encounter any problems or have a feature request, please [open an issue](https://github.com/morhodesly/database-dump/issues).

## Roadmap

- [ ] Add support for schema filtering
- [ ] Add support for table filtering
- [ ] Add data-only and schema-only dump modes
- [ ] Implement environment variable support for credentials
- [ ] Create pre-built binaries for common platforms

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details. 