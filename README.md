# McGravity

<div align="center">
  <img src="./assets/thumbnail.png" alt="McGravity Thumbnail" width="200">
</div>

## About
McGravity is a tool that connects multiple MCP (Machine Communication Protocol) servers into one unified service. It lets you reuse the same MCP server and scale underlying MCP server connections almost infinitely.

The current version works as a basic CLI tool, but McGravity will grow to become a full-featured proxy for MCP servers - like Nginx but for modern Gen AI tools and servers.

## Why McGravity?

```
Without McGravity:
┌─────────┐     ┌─────────┐
│ Client  │────▶│MCP      │
│         │     │Server 1 │
└─────────┘     └─────────┘
    │               
    │           ┌─────────┐
    └──────────▶│MCP      │
                │Server 2 │
                └─────────┘
```

```
With McGravity:
┌─────────┐     ┌─────────┐     ┌─────────┐
│ Client  │────▶│McGravity│────▶│MCP      │
│         │     │         │     │Server 1 │
└─────────┘     └─────────┘     └─────────┘
                     │          
                     │          ┌─────────┐
                     └─────────▶│MCP      │
                                │Server 2 │
                                └─────────┘
```

McGravity solves these problems:
- Connect to multiple MCP servers through one endpoint
- Balance load between MCP servers
- Provide a single point of access for your applications

## Installation

```bash
# Install dependencies
bun install

# Build the project into a single executable
bun build src/index.ts --compile --outfile mcgravity
```

## Usage

Basic command:
```bash
./mcgravity <mcp-server-address1> <mcp-server-address2> ...
```

With options:
```bash
./mcgravity --host localhost --port 3001 http://mcp1.example.com http://mcp2.example.com
```

Using configuration file:
```bash
./mcgravity --config config.yaml
```

### Options

- `--host <host>`: Host to bind the server to (default: localhost)
- `--port <port>`: Port to bind the server to (default: 3001)
- `--config <path>`: Path to the config file (default: config.yaml)
- `--mcp-version <version>`: Version of the MCP server (default: 1.0.0)
- `--mcp-name <name>`: Name of the MCP server (default: mcgravity)
- `--help`: Show help information

### Configuration

McGravity can be configured using a YAML file. See `config.example.yaml` for a sample configuration:

```yaml
name: mcgravity
version: 1.0.0
description: A simple MCP server

servers:
  echo-server:
    url: http://localhost:3000/sse
    name: echo-server
    version: 1.0.0
    description: A simple echo server
    tags:
      - echo
```

You can run the included echo server example for testing:

```bash
# Start the echo server first
bun examples/echo-server.ts

# Then start McGravity pointing to the echo server
./mcgravity --config config.yaml
```

## Examples

Start McGravity with default settings:
```bash
./mcgravity http://mcp1.example.com http://mcp2.example.com
```

Specify host and port:
```bash
./mcgravity --host 0.0.0.0 --port 4000 http://mcp1.example.com http://mcp2.example.com
```

## Running Tests

To run all tests:

```bash
bun test
```

To run integration tests only:

```bash
bun run test:integration
```

### Integration Tests

The integration tests verify that McGravity can:

1. Connect to an MCP server (the example echo server)
2. Correctly proxy capabilities from the target MCP server
3. Pass requests from clients to the target MCP server and return responses

For more details about the test suite, see the [test README](test/README.md).

The tests are automatically run in GitHub Actions CI on push and PR events.

## Future Plans

McGravity will expand to include:
- Web interface for monitoring
- Advanced load balancing
- MCP server health checks
- Authentication and access control
- Plugin system for custom integrations

## Contributing

Contributions are welcome! Feel free to open issues or submit pull requests.
