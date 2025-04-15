# McGravity Docker Guide

## Overview

McGravity is a tool that connects multiple MCP (Machine Communication Protocol) servers into one unified service. This document explains how to use McGravity with Docker.

## Using the Docker Image

### Pull the Docker Image

```bash
docker pull tigranbs/mcgravity
```

### Run McGravity with Docker

Basic usage:

```bash
docker run -p 3001:3001 tigranbs/mcgravity http://mcp1.example.com http://mcp2.example.com
```

With custom host and port:

```bash
docker run -p 4000:4000 tigranbs/mcgravity --host 0.0.0.0 --port 4000 http://mcp1.example.com
```

### Using a Configuration File

1. Create a config.yaml file locally
2. Mount it to the container:

```bash
docker run -p 3001:3001 -v $(pwd)/config.yaml:/app/config.yaml tigranbs/mcgravity --config /app/config.yaml
```

## Building the Docker Image Locally

1. Clone the repository:

```bash
git clone https://github.com/tigranbs/mcgravity.git
cd mcgravity
```

2. Build the Docker image:

```bash
docker build -t mcgravity .
```

3. Run your local build:

```bash
docker run -p 3001:3001 mcgravity
```

## Docker Compose Example

```yaml
services:
  mcgravity:
    image: tigranbs/mcgravity:latest
    ports:
      - '3001:3001'
    volumes:
      - ./config.yaml:/app/config.yaml
    command: --config /app/config.yaml
    restart: unless-stopped
```

## Container Details

- The McGravity Docker image is based on Alpine Linux for minimal size
- The application is compiled to a single binary for optimal performance
- Default port exposed: 3001
- Default command: `--help`

## Environment Variables

None currently supported. All configuration is done via command-line arguments or the config file.

## Troubleshooting

If you encounter issues:

1. Ensure ports are correctly mapped
2. Check that your config file is correctly mounted
3. Verify MCP server URLs are accessible from the container

For more details about McGravity itself, see the main [README.md](https://github.com/tigranbs/mcgravity/blob/main/README.md).
