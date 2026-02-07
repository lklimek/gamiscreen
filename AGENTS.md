## Code Standards

### Required Before Each Commit
- Run `cargo +nightly fmt` before committing any changes to ensure proper code formatting
- This will format all Rust files according to the project's rustfmt configuration

### Development Flow - keep the order:
1. Before writing any code:
  1. evaluate provided request and ask clarifying questions if needed
  2. review documents in docs/, 
  3. review existing code, 
  4. find design patterns already in use in the code that can be used
  5. identify tasks to execute and write them don in docs/todos/ (markdown formatted TODO list with checkboxes)
2. Iterate on the identified tasks, marking completed as done in the todo file
3. Check code quality with `cargo clippy`, `cargo +nightly fmt` and  `cargo test` or equivalent tools
4. Update documentation: review README.md and  in docs/*.md and update them accordingly
5. Final self-review: check all the changes, verify if new code doesn't duplicate existing functions, find and report any gaps


## Repository Structure
- android/ - Android app (embedding gamiscreen-web)
- dist/ - output for generated artifacts
- gamiscreen-server - server
- gamiscreen-client - client for devices that are managed by the server
- gamiscreen-web - management interface; embedded into android app
- gamiscreen-shared - components needed to communicate with gamiscreen-web, mainly API and data model-related
- scripts - development tools


## Key Guidelines
1. Follow Rust best practices and idiomatic patterns.
2. Don't repeat yourself - always check if related or similar code already exists.
3. Maintain existing code structure and organization.
4. Write unit tests for new functionality.
5. Write a TODO comment whenever some gap is identified.
6. Use conventional commits.

