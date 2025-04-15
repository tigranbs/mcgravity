# McGravity Testing

This directory contains tests for the McGravity application.

## Structure

- `integration/`: Integration tests that test McGravity with other MCP servers
- `run-integration-tests.ts`: Script to run all integration tests

## Running Tests

To run all tests:

```bash
bun test
```

To run integration tests only:

```bash
bun run test:integration
```

## Integration Tests

The integration tests verify that McGravity can:

1. Connect to an MCP server (the example echo server)
2. Correctly proxy capabilities from the target MCP server
3. Pass requests from clients to the target MCP server and return responses

### Echo Server Test

The `echo-server.test.ts` file tests integration with the example echo server. It:

1. Starts the echo server from the `examples` directory
2. Creates a McGravity server that connects to the echo server
3. Creates a client that connects to McGravity
4. Tests that the client can:
   - List tools (including those from the echo server)
   - Call the echo tool through McGravity

## CI Integration

These tests are automatically run as part of the GitHub Actions CI pipeline. The workflow runs on:
- Push events to the main branch
- Pull request events targeting the main branch
- Manual triggers via workflow_dispatch

The GitHub workflow configuration can be found in `.github/workflows/integration-tests.yml`.

## Adding More Tests

To add a new integration test:

1. Create a new `.test.ts` file in the `integration` directory
2. Use the Bun test framework (`import { describe, expect, test } from "bun:test"`)
3. Follow the pattern in the existing tests:
   - Start necessary servers in `beforeAll`
   - Clean up servers in `afterAll`
   - Write test cases that verify McGravity's functionality 