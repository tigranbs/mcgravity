import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { McpServer, type ResourceMetadata } from '@modelcontextprotocol/sdk/server/mcp.js';
import type {
  Implementation,
  Tool,
  CallToolResult,
  Resource,
  Prompt,
} from '@modelcontextprotocol/sdk/types.js';
import { SSEClientTransport } from '@modelcontextprotocol/sdk/client/sse.js';
import { jsonSchemaToZod } from './utils/schema-converter';

export class McpServerComposer {
  public readonly server: McpServer;
  private readonly targetClients: Map<
    string,
    {
      clientInfo: Implementation;
      url: URL;
    }
  > = new Map();

  constructor(serverInfo: Implementation) {
    this.server = new McpServer(serverInfo);
  }

  async addTargetServer(
    targetServerUrl: URL,
    clientInfo: Implementation,
    skipRegister = false
  ): Promise<void> {
    const targetClient = new Client(clientInfo);
    const targetTransport = new SSEClientTransport(targetServerUrl);
    try {
      await targetClient.connect(targetTransport);
    } catch (error) {
      console.error(
        `Failed to connect to ${targetServerUrl} -> ${clientInfo.name}`,
        (error as Error).message
      );

      // If the connection fails, retry after 10 seconds
      return new Promise((resolve) => {
        setTimeout(() => {
          resolve(this.addTargetServer(targetServerUrl, clientInfo, skipRegister));
        }, 10000);
      });
    }

    console.log(`Connected to ${targetServerUrl} -> ${clientInfo.name}`);

    const name = targetServerUrl.toString();

    this.targetClients.set(name, { clientInfo, url: targetServerUrl });

    if (skipRegister) {
      console.log(`Skipping capabilities registration for ${name}`);
      return;
    }

    console.log(`Registering capabilities for ${name}`);
    const tools = await targetClient.listTools();
    this.composeTools(tools.tools, name);
    console.log(`Registered ${tools.tools.length} tools for ${name}`);

    const resources = await targetClient.listResources();
    this.composeResources(resources.resources, name);
    console.log(`Registered ${resources.resources.length} resources for ${name}`);

    const prompts = await targetClient.listPrompts();
    this.composePrompts(prompts.prompts, name);
    console.log(`Registered ${prompts.prompts.length} prompts for ${name}`);

    console.log(`Capabilities registration for ${name} completed`);
    targetClient.close(); // We don't have to keep the client open
  }

  listTargetClients() {
    return Array.from(this.targetClients.values());
  }

  async disconnectAll() {
    for (const client of this.targetClients.keys()) {
      await this.disconnect(client);
    }
  }

  async disconnect(clientName: string) {
    const client = this.targetClients.get(clientName);
    if (client) {
      this.targetClients.delete(clientName);
    }
  }

  private composeTools(tools: Tool[], name: string) {
    for (const tool of tools) {
      const schemaObject = jsonSchemaToZod(tool.inputSchema);
      this.server.tool(tool.name, tool.description ?? '', schemaObject, async (args) => {
        const clientItem = this.targetClients.get(name);
        if (!clientItem) {
          throw new Error(`Client for ${name} not found`);
        }

        const client = new Client(clientItem.clientInfo);
        await client.connect(new SSEClientTransport(clientItem.url));
        console.log(`Calling tool ${tool.name} with args ${JSON.stringify(args)}`);

        const result = await client.callTool({
          name: tool.name,
          arguments: args,
        });
        await client.close();
        return result as CallToolResult;
      });
    }
  }

  private composeResources(resources: Resource[], name: string) {
    for (const resource of resources) {
      this.server.resource(
        resource.name,
        resource.uri,
        { description: resource.description, mimeType: resource.mimeType },
        async (uri) => {
          const clientItem = this.targetClients.get(name);
          if (!clientItem) {
            throw new Error(`Client for ${name} not found`);
          }

          const client = new Client(clientItem.clientInfo);
          await client.connect(new SSEClientTransport(clientItem.url));

          const result = await client.readResource({
            uri: uri.toString(),
            _meta: resource._meta as ResourceMetadata,
          });
          await client.close();
          return result;
        }
      );
    }
  }

  private composePrompts(prompts: Prompt[], name: string) {
    for (const prompt of prompts) {
      const argsSchema = jsonSchemaToZod(prompt.arguments);
      this.server.prompt(prompt.name, prompt.description ?? '', argsSchema, async (args) => {
        const clientItem = this.targetClients.get(name);
        if (!clientItem) {
          throw new Error(`Client for ${name} not found`);
        }

        const client = new Client(clientItem.clientInfo);
        await client.connect(new SSEClientTransport(clientItem.url));

        const result = await client.getPrompt({
          name: prompt.name,
          arguments: args,
        });
        await client.close();
        return result;
      });
    }
  }

  private handleTargetServerClose(name: string, targetServerUrl: URL, clientInfo: Implementation) {
    return () => {
      this.targetClients.delete(name);
      console.error(
        `Disconnected from ${name} [${targetServerUrl}] -> ${clientInfo.name}. Retrying in 10 seconds...`
      );
      return this.addTargetServer(targetServerUrl, clientInfo, true);
    };
  }
}
