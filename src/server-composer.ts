import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import {
  McpServer,
  type PromptCallback,
  type ResourceMetadata,
} from "@modelcontextprotocol/sdk/server/mcp.js";
import type {
  Implementation,
  Tool,
  CallToolResult,
  Resource,
  Prompt,
} from "@modelcontextprotocol/sdk/types.js";
import { SSEClientTransport } from "@modelcontextprotocol/sdk/client/sse.js";
import { jsonSchemaToZod } from "./utils/schema-converter";

export class McpServerComposer {
  public readonly server: McpServer;
  private readonly targetClients: Map<string, Client> = new Map();

  constructor(serverInfo: Implementation) {
    this.server = new McpServer(serverInfo);
  }

  async addTargetServer(targetServerUrl: URL, clientInfo: Implementation, skipRegister = false): Promise<void> {
    const targetClient = new Client(clientInfo);
    try {
      await targetClient.connect(new SSEClientTransport(targetServerUrl));
    } catch (error) {
      console.error(`Failed to connect to ${targetServerUrl} -> ${clientInfo.name}`, (error as Error).message);

      // If the connection fails, retry after 10 seconds
      return new Promise((resolve) => {
        setTimeout(() => {
          resolve(this.addTargetServer(targetServerUrl, clientInfo, skipRegister));
        }, 10000);
      });
    }

    console.log(`Connected to ${targetServerUrl} -> ${clientInfo.name}`);

    const name = targetServerUrl.toString();

    targetClient.onclose = this.handleTargetServerClose(name, targetServerUrl, clientInfo);
    targetClient.onerror = this.handleTargetServerClose(name, targetServerUrl, clientInfo);

    this.targetClients.set(name, targetClient);

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
      await client.close();
      this.targetClients.delete(clientName);
    }
  }

  private composeTools(tools: Tool[], name: string) {
    for (const tool of tools) {
      const schemaObject = jsonSchemaToZod(tool.inputSchema);
      this.server.tool(
        tool.name,
        tool.description ?? "",
        schemaObject,
        async (args, extra) => {
          const client = this.targetClients.get(name);
          if (!client) {
            throw new Error(`Client for ${name} not found`);
          }

          const result = await client.callTool({
            name: tool.name,
            arguments: args,
          });
          return result as CallToolResult;
        }
      );
    }
  }

  private composeResources(resources: Resource[], name: string) {
    for (const resource of resources) {
      this.server.resource(
        resource.name,
        resource.uri,
        { description: resource.description, mimeType: resource.mimeType },
        async (uri, extra) => {
          const client = this.targetClients.get(name);
          if (!client) {
            throw new Error(`Client for ${name} not found`);
          }

          return await client.readResource({
            uri: uri.toString(),
            _meta: resource._meta as ResourceMetadata,
          });
        }
      );
    }
  }

  private composePrompts(prompts: Prompt[], name: string) {
    for (const prompt of prompts) {
      const argsSchema = jsonSchemaToZod(prompt.arguments);
      this.server.prompt(
        prompt.name,
        prompt.description ?? "",
        argsSchema,
        async (args, extra) => {
          const client = this.targetClients.get(name);
          if (!client) {
            throw new Error(`Client for ${name} not found`);
          }

          return await client.getPrompt({
            name: prompt.name,
            arguments: args,
          });
        }
      );
    }
  }

  private handleTargetServerClose(name: string, targetServerUrl: URL, clientInfo: Implementation) {
    return () => {
      this.targetClients.delete(name);
      console.error(`Disconnected from ${name} [${targetServerUrl}] -> ${clientInfo.name}. Retrying in 10 seconds...`);
      return this.addTargetServer(targetServerUrl, clientInfo, true);
    };
  }
}
