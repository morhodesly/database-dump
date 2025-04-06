# PostgreSQL Database Dump Utility

A Rust-based utility for creating comprehensive PostgreSQL database dumps with proper handling of users, roles, permissions, schemas, and data.

## Keywords

- postgresql dump tool
- database export utility
- postgres backup
- postgresql migration tool
- sql dump creator
- database backup utility
- postgres schema exporter
- rust database tools
- postgres data migration
- postgresql role permissions
- database schema dump
- pg role export
- postgresql backup solution
- database cloning tool
- selective postgresql dump

## Short Description

A robust command-line tool for exporting PostgreSQL databases with proper role handling and dependency ordering.

## Long Description

This PostgreSQL dump utility is a specialized Rust-based tool designed to overcome common limitations in standard pg_dump tools. It exports complete database structures including tables, data, sequences, types, users, roles, and their permissions - all properly ordered for seamless reimporting.

What makes this tool unique:

1. **Role Handling**: Only exports users and roles directly associated with the database
2. **Proper Ordering**: Creates dumps with correct dependency ordering for clean imports
3. **Type Support**: Properly handles custom data types and enums
4. **Selective Export**: Focuses on relevant database objects
5. **Cross-Platform**: Runs on Windows, macOS, and Linux

Ideal for database migrations, backups, development environment setup, and creating consistent database snapshots. 