import { NextResponse, type NextRequest } from "next/server";
import { ZodError, type ZodType } from "zod";

/** Wire shape of every failure: `{ error, code? }`, never a stack trace. */
export interface ErrorBody {
  error: string;
  code?: string;
  [extra: string]: unknown;
}

/**
 * An error the caller is allowed to see. Anything else that escapes a handler
 * is logged server-side and answered with a flat 500.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly code?: string;
  readonly extra?: Record<string, unknown>;

  constructor(
    status: number,
    message: string,
    code?: string,
    extra?: Record<string, unknown>,
  ) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.extra = extra;
  }
}

export const unauthorized = (message = "authentication required") =>
  new ApiError(401, message, "unauthenticated");
export const forbidden = (message = "not allowed") =>
  new ApiError(403, message, "forbidden");
export const notFound = (message = "not found") =>
  new ApiError(404, message, "not_found");
export const conflict = (
  message: string,
  extra?: Record<string, unknown>,
  code = "conflict",
) => new ApiError(409, message, code, extra);
export const badRequest = (message: string, code = "bad_request") =>
  new ApiError(400, message, code);

/** Flattens zod issues into one readable line: `title: too small`. */
export function formatZodError(error: ZodError): string {
  return error.issues
    .map((issue) => {
      const path = issue.path.join(".");
      return path ? `${path}: ${issue.message}` : issue.message;
    })
    .join("; ");
}

export function errorResponse(error: unknown): NextResponse<ErrorBody> {
  if (error instanceof ApiError) {
    const body: ErrorBody = { error: error.message };
    if (error.code) body.code = error.code;
    return NextResponse.json({ ...body, ...error.extra }, { status: error.status });
  }
  if (error instanceof ZodError) {
    return NextResponse.json(
      { error: formatZodError(error), code: "invalid_payload" },
      { status: 400 },
    );
  }
  // Unexpected: the details belong in the platform logs, not in the response.
  console.error("[hangar-cloud] unhandled route error", error);
  return NextResponse.json({ error: "internal error", code: "internal" }, { status: 500 });
}

type Handler = (request: NextRequest) => Promise<Response>;
type ParamHandler<P> = (
  request: NextRequest,
  context: { params: Promise<P> },
) => Promise<Response>;

/** Wraps a handler on a static path so thrown ApiErrors become JSON. */
export function route(handler: Handler): Handler {
  return async (request) => {
    try {
      return await handler(request);
    } catch (error) {
      return errorResponse(error);
    }
  };
}

/** Same, for a path with dynamic segments (`params` is a promise in Next 15+). */
export function paramRoute<P>(handler: ParamHandler<P>): ParamHandler<P> {
  return async (request, context) => {
    try {
      return await handler(request, context);
    } catch (error) {
      return errorResponse(error);
    }
  };
}

/** Parses a JSON body with zod, turning both failures into a 400. */
export async function readJson<T>(request: NextRequest, schema: ZodType<T>): Promise<T> {
  let raw: unknown;
  try {
    raw = await request.json();
  } catch {
    throw badRequest("body must be valid JSON", "invalid_json");
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw badRequest(formatZodError(parsed.error), "invalid_payload");
  }
  return parsed.data;
}

/** Reads and validates the query string with the same error shape as bodies. */
export function readQuery<T>(request: NextRequest, schema: ZodType<T>): T {
  const parsed = schema.safeParse(Object.fromEntries(request.nextUrl.searchParams));
  if (!parsed.success) {
    throw badRequest(formatZodError(parsed.error), "invalid_query");
  }
  return parsed.data;
}

export function json<T>(body: T, init?: ResponseInit): NextResponse<T> {
  return NextResponse.json(body, init);
}
