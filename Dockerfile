FROM oven/bun:alpine AS builder

WORKDIR /app

# Copy package files
COPY package.json bun.lock ./

# Install dependencies
RUN bun install --frozen-lockfile
# Copy source code
COPY . .

RUN bun run format:check
RUN bun run lint

# Build the project into a single executable
RUN bun build ./src/index.ts --compile --minify --sourcemap --bytecode --outfile mcgravity

# Use a minimal alpine image for the final container
FROM alpine:latest

WORKDIR /app

# Install necessary runtime dependencies - add C++ libraries
RUN apk add --no-cache \
    libc6-compat \
    libstdc++ \
    libgcc \
    ca-certificates

# Copy only the compiled binary from the builder stage
COPY --from=builder /app/mcgravity .

# Make the binary executable
RUN chmod +x /app/mcgravity

# Set up entrypoint
ENTRYPOINT ["/app/mcgravity"]

# Default command - can be overridden at runtime
CMD ["--help"]
