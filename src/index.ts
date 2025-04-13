#!/usr/bin/env bun

import { parseArgs } from "node:util";
import { McGravityServer } from "./server";

interface CliOptions {
  host: string;
  port: number;
  help: boolean;
  mcpServers: string[];
}

function showHelp() {
  console.log(`
mcgravity - MCP Gravity tool

Usage:
  mcgravity --host <server host> --port <server port> <mcp-server-address1>... <mcp server address-n>

Options:
  --host <host>       Host to bind the server to (default: localhost)
  --port <port>       Port to bind the server to (default: 3001)
  --help              Show this help message

Examples:
  mcgravity --host localhost --port 3001 http://mcp1.example.com http://mcp2.example.com
`);
}

function parseCliOptions(): CliOptions {
  const { values, positionals } = parseArgs({
    options: {
      host: {
        type: "string",
        short: "h",
        default: "localhost",
      },
      port: {
        type: "string",
        short: "p",
        default: "3001",
      },
      help: {
        type: "boolean",
        short: "?",
        default: false,
      },
    },
    allowPositionals: true,
  });

  return {
    host: values.host || "localhost",
    port: parseInt(values.port as string) || 3001,
    help: values.help || false,
    mcpServers: positionals,
  };
}

async function main() {
  const options = parseCliOptions();

  if (options.help || options.mcpServers.length === 0) {
    showHelp();
    process.exit(options.help ? 0 : 1);
  }

  const server = new McGravityServer({
    name: "mcgravity",
    version: "1.0.0",
  }, {
      port: options.port,
      host: options.host,
    }
  );

  await server.loadTargets(options.mcpServers);
  server.start();
  console.log("Server started on", options.host, options.port);
}

main().catch((error) => {
  console.error("Error:", error);
  process.exit(1);
});
