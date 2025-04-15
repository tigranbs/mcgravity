import { describe, expect, test, beforeAll, afterAll } from 'bun:test';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { SSEClientTransport } from '@modelcontextprotocol/sdk/client/sse.js';
import { McGravityServer } from '../../src/server';
import { spawn, type ChildProcess } from 'child_process';

describe('McGravity integration with Echo Server', () => {
  let echoServerProcess: ChildProcess;
  let mcGravityServer: McGravityServer;
  const mcGravityPort = 3001;
  const echoServerPort = 3000;
  let client: Client;

  beforeAll(async () => {
    // Start the echo server
    echoServerProcess = spawn('bun', ['examples/echo-server.ts'], {
      stdio: 'pipe',
    });

    // Wait for echo server to start
    await new Promise((resolve) => setTimeout(resolve, 2000));

    // Start McGravity server
    mcGravityServer = new McGravityServer(
      {
        name: 'mcgravity-test',
        version: '1.0.0',
        description: 'Test McGravity server',
      },
      {
        port: mcGravityPort,
        host: 'localhost',
      }
    );

    // Connect to the echo server
    await mcGravityServer.loadTargets([
      {
        url: `http://localhost:${echoServerPort}/sse`,
        name: 'echo-server-test',
        version: '1.0.0',
      },
    ]);

    // Start the server
    mcGravityServer.start();

    // Wait for McGravity server to start and register all capabilities
    await new Promise((resolve) => setTimeout(resolve, 3000));

    // Create a client to connect to McGravity
    client = new Client({
      name: 'test-client',
      version: '1.0.0',
    });

    await client.connect(new SSEClientTransport(new URL(`http://localhost:${mcGravityPort}/`)));

    // Give some time for the client connection to stabilize
    await new Promise((resolve) => setTimeout(resolve, 1000));
  });

  afterAll(async () => {
    try {
      // Close the client connection
      if (client) {
        await client.close();
      }
    } catch (error) {
      console.error('Error closing client:', error);
    }

    // Kill the echo server
    if (echoServerProcess && echoServerProcess.kill) {
      echoServerProcess.kill();
    }

    // Wait for processes to clean up
    await new Promise((resolve) => setTimeout(resolve, 1000));
  });

  test('should be able to list tools through McGravity', async () => {
    // List tools to verify the echo tool is available
    const tools = await client.listTools();

    // Verify at least one tool is registered
    expect(tools.tools.length).toBeGreaterThan(0);

    // Find the echo tool
    const echoTool = tools.tools.find((tool) => tool.name === 'echo');
    expect(echoTool).toBeDefined();
  });

  test('should be able to call the echo tool through McGravity', async () => {
    // Call the echo tool with a test message
    const testMessage = 'Hello from integration test';
    const result = await client.callTool({
      name: 'echo',
      arguments: {
        message: testMessage,
      },
    });

    // Verify the response has content
    expect(result).toBeDefined();
    expect(result).toHaveProperty('content');

    // Type assertion to access content properties safely
    const content = result.content as Array<{ type: string; text: string }>;
    expect(content).toHaveLength(1);
    expect(content[0]?.type).toBe('text');
    expect(content[0]?.text).toBe(`Tool echo: ${testMessage}`);
  });
});
