# McGravity

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

### Options

- `--host <host>`: Host to bind the server to (default: localhost)
- `--port <port>`: Port to bind the server to (default: 3001)
- `--help`: Show help information

## Examples

Start McGravity with default settings:
```bash
./mcgravity http://mcp1.example.com http://mcp2.example.com
```

Specify host and port:
```bash
./mcgravity --host 0.0.0.0 --port 4000 http://mcp1.example.com http://mcp2.example.com
```

## Future Plans

McGravity will expand to include:
- Web interface for monitoring
- Advanced load balancing
- MCP server health checks
- Authentication and access control
- Plugin system for custom integrations

## Contributing

Contributions are welcome! Feel free to open issues or submit pull requests.
