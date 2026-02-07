## Code Standards

### Required Before Each Commit
- Run `make fmt` before committing any changes to ensure proper code formatting
- This will run gofmt on all Go files to maintain consistent style

### Development Flow
- Analysis:
  -  evaluate provided task and ask clarifying questions if needed
  - review documents in docs/, 
  - review existing code, 
  - find design patterns that apply for the task
  -  generate a new file in docs/todos/ with a  TODO list (markdown format with checkboxes)
- Implementation
- Code quality always use `cargo clippy`, `cargo fmt` and   `cargo test` or equivalent tools
- Documentation: review README.md and  in docs/*.md and update them accordingly
- Final self-review: check all the changes, verify if new code doesn't duplicate existing functions, find and report any gaps

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

