import { randomUUID } from 'node:crypto';
import { JSONRPCMessageSchema, type JSONRPCMessage } from '@modelcontextprotocol/sdk/types.js';

/**
 * Server transport for SSE using Bun's Response type.
 * Adapts the SSEServerTransport functionality to work with Bun.
 */
export class BunSSEServerTransport {
  private _sessionId: string;
  private _sseResponse?: Response;
  private _responseObj?: Response;
  private _writer?: WritableStreamDefaultWriter<Uint8Array>;
  onmessage?: (message: JSONRPCMessage) => void;
  onclose?: () => void;
  onerror?: (error: Error) => void;

  /**
   * Creates a new SSE server transport for Bun.
   * @param _endpoint The endpoint where clients should POST messages
   */
  constructor(private _endpoint: string) {
    this._sessionId = randomUUID();
  }

  /**
   * Creates a Response object suitable for Bun.serve to return.
   * This method should be called to get the Response for the initial SSE request.
   */
  createResponse(): Promise<Response> {
    if (this._responseObj) {
      return Promise.resolve(this._responseObj);
    }

    // Create a readable stream that we'll write SSE events to
    const { readable, writable } = new TransformStream();
    this._writer = writable.getWriter();

    // Write the initial headers
    const encoder = new TextEncoder();
    this._writer.write(
      encoder.encode(
        `event: endpoint\ndata: ${encodeURI(this._endpoint)}?sessionId=${this._sessionId}\n\n`
      )
    );

    // Create the response
    this._responseObj = new Response(readable, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache, no-transform',
        Connection: 'keep-alive',
      },
    });

    return Promise.resolve(this._responseObj);
  }

  /**
   * Start the SSE connection - required by McpServer
   * Note: Does not return a Response, unlike createResponse
   */
  async start(): Promise<void> {
    if (!this._responseObj) {
      await this.createResponse();
    }
    this._sseResponse = this._responseObj;
  }

  /**
   * Handles incoming POST messages.
   */
  async handlePostMessage(req: Request): Promise<Response> {
    if (!this._sseResponse) {
      const message = 'SSE connection not established';
      return new Response(message, { status: 500 });
    }

    try {
      const contentTypeHeader = req.headers.get('content-type');
      if (!contentTypeHeader || !contentTypeHeader.includes('application/json')) {
        throw new Error(`Unsupported content-type: ${contentTypeHeader}`);
      }

      const body = await req.json();
      await this.handleMessage(body as JSONRPCMessage);

      return new Response('Accepted', { status: 202 });
    } catch (error) {
      this.onerror?.(error as Error);
      return new Response(String(error), { status: 400 });
    }
  }

  /**
   * Handle a client message, regardless of how it arrived.
   */
  async handleMessage(message: JSONRPCMessage) {
    try {
      const parseResult = JSONRPCMessageSchema.safeParse(message);
      if (parseResult.success) {
        this.onmessage?.(parseResult.data);
      } else {
        throw new Error(`Invalid JSON-RPC message: ${parseResult.error.message}`);
      }
    } catch (error) {
      this.onerror?.(error as Error);
      throw error;
    }
  }

  /**
   * Close the SSE connection.
   */
  async close() {
    if (this._writer) {
      await this._writer.close();
      this._writer = undefined;
    }
    this._sseResponse = undefined;
    this._responseObj = undefined;
    this.onclose?.();
  }

  /**
   * Send a message over the SSE connection.
   */
  async send(message: JSONRPCMessage) {
    if (!this._writer) {
      throw new Error('Not connected');
    }

    const encoder = new TextEncoder();
    this._writer.write(encoder.encode(`event: message\ndata: ${JSON.stringify(message)}\n\n`));
  }

  /**
   * Returns the session ID for this transport.
   */
  get sessionId(): string {
    return this._sessionId;
  }
}
