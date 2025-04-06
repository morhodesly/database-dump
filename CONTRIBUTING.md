# Contributing to PostgreSQL Database Dump Utility

Thank you for your interest in contributing to the PostgreSQL Database Dump Utility! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to act with respect towards other contributors and maintain a constructive atmosphere. Please be patient and considerate when interacting with others.

## How Can I Contribute?

### Reporting Bugs

- Check the [issue tracker](https://github.com/yourusername/database-dump/issues) to see if the bug has already been reported
- If it hasn't, [open a new issue](https://github.com/yourusername/database-dump/issues/new/choose) using the bug report template
- Include a clear title and description
- Provide as much relevant information as possible (steps to reproduce, expected vs. actual behavior)
- Include code samples or error logs if applicable

### Suggesting Features

- Check the [issue tracker](https://github.com/yourusername/database-dump/issues) to see if the feature has already been suggested
- If it hasn't, [open a new issue](https://github.com/yourusername/database-dump/issues/new/choose) using the feature request template
- Describe the feature clearly, including the problem it solves
- Explain how the feature would benefit users

### Pull Requests

1. Fork the repository
2. Create a new branch for your feature or bug fix
3. Make your changes
4. Add or update tests as needed
5. Ensure all tests pass: `cargo test`
6. Format your code: `cargo fmt`
7. Check for linting issues: `cargo clippy`
8. Commit your changes with a clear, descriptive message
9. Push your branch to your fork
10. Open a pull request against the main branch

## Development Setup

1. Install Rust and Cargo from [rustup.rs](https://rustup.rs/)
2. Clone the repository: `git clone https://github.com/yourusername/database-dump.git`
3. Navigate to the project directory: `cd database-dump`
4. Install development dependencies: `cargo build`
5. For testing, you'll need a PostgreSQL database

## Coding Guidelines

- Follow the [Rust Style Guide](https://github.com/rust-lang/rustfmt)
- Write clear, concise code with meaningful variable and function names
- Add comments for complex logic
- Include documentation for public API functions
- Write tests for new functionality

## Commit Message Guidelines

- Use the present tense ("Add feature" not "Added feature")
- Use the imperative mood ("Move cursor to..." not "Moves cursor to...")
- First line should be 50 characters or less
- Reference issues and pull requests where appropriate

## Testing

- Add tests for new functionality
- Ensure all tests pass before submitting a pull request
- Consider edge cases in your tests

## Questions?

If you have any questions or need help, please open an issue or contact the maintainers.

Thank you for contributing! 