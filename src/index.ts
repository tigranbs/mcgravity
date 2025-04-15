#!/usr/bin/env bun

import { parseArgs } from 'node:util';
import { McGravityServer } from './server';
import { loadConfig } from './utils/config';
import type { ConfigType, McpServerType } from './utils/schemas';

interface CliOptions {
  host: string;
  port: number;
  help: boolean;
  mcpServers: string[];
  mcpVersion: string;
  mcpName: string;
  config?: string;
}

function showHelp() {
  console.log(`
mcgravity - MCP Gravity tool

Usage:
  mcgravity --host <server host> --port <server port> <mcp-server-address1>... <mcp server address-n>

Options:
  --host <host>       Host to bind the server to (default: localhost)
  --port <port>       Port to bind the server to (default: 3001)
  --config <path>     Path to the config file (default: config.yaml)
  --mcp-version <version>   Version of the MCP server (default: 1.0.0)
  --mcp-name <name>       Name of the MCP server (default: hostname)
  --help              Show this help message

Examples:
  mcgravity --host localhost --port 3001 http://mcp1.example.com http://mcp2.example.com
  mcgravity --config config.yaml
`);
}

async function parseCliOptions(): Promise<CliOptions> {
  const { values, positionals } = parseArgs({
    options: {
      host: {
        type: 'string',
        short: 'h',
        default: 'localhost',
      },
      port: {
        type: 'string',
        short: 'p',
        default: '3001',
      },
      config: {
        type: 'string',
        short: 'c',
        default: 'config.yaml',
      },
      help: {
        type: 'boolean',
        short: '?',
        default: false,
      },
      'mcp-version': {
        type: 'string',
        short: 'v',
        default: '1.0.0',
      },
      'mcp-name': {
        type: 'string',
        short: 'n',
        default: 'mcgravity',
      },
    },
    allowPositionals: true,
  });

  let configFile: string | undefined = values.config;
  if (!configFile) {
    configFile = 'config.yaml';
  }
  const configExists = await Bun.file(configFile).exists();
  if (!configExists) {
    configFile = undefined;
  }

  return {
    host: values.host || 'localhost',
    port: parseInt(values.port as string) || 3001,
    help: values.help || false,
    mcpServers: positionals,
    mcpVersion: values['mcp-version'] || '1.0.0',
    mcpName: values['mcp-name'] || 'mcgravity',
    config: configFile,
  };
}

async function main() {
  const options = await parseCliOptions();

  let mcpServers: McpServerType[] = [];
  let mcGravityConfig: ConfigType | undefined;

  if (options.config) {
    mcGravityConfig = await loadConfig(options.config);
  }

  if (options.help || options.mcpServers.length === 0) {
    if (!mcGravityConfig) {
      showHelp();
      process.exit(options.help ? 0 : 1);
    }

    mcpServers = Object.values(mcGravityConfig.servers);
  } else {
    mcpServers = options.mcpServers.map((server) => ({
      url: server,
      name: new URL(server).hostname,
      version: options.mcpVersion,
    }));
  }

  const server = new McGravityServer(
    {
      name: mcGravityConfig?.name || options.mcpName,
      version: mcGravityConfig?.version || options.mcpVersion,
      description: mcGravityConfig?.description || '',
    },
    {
      port: options.port,
      host: options.host,
    }
  );

  await server.loadTargets(mcpServers);
  server.start();
  console.log('Server started on', options.host, options.port);
}

main().catch((error) => {
  console.error('Error:', error);
  process.exit(1);
});
