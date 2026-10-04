import { Catch, HttpException } from '@nestjs/common'
import type { ArgumentsHost, ExceptionFilter } from '@nestjs/common'
import type { Response } from 'express'

const REQUEST_TOO_LARGE = 413

const statuses: Record<string, number> = {
  bad_request: 400,
  invalid_request: 400,
  unauthorized: 401,
  invalid_token: 401,
  device_not_found: 404,
  not_found: 404,
  conflict: 409,
  revision_conflict: 409,
  regions_missing: 409,
  device_retired: 410,
  retired: 410,
  storage_failure: 503
}

export function errorResponse(error: unknown): { status: number; body: { code: string; message: string } } {
  if (error instanceof Error && 'code' in error && typeof error.code === 'string' && statuses[error.code]) {
    return { status: statuses[error.code], body: { code: error.code, message: error.message } }
  }
  if (error instanceof HttpException) {
    const status = error.getStatus()
    return { status, body: { code: status === REQUEST_TOO_LARGE ? 'request_too_large' : 'invalid_request', message: error.message } }
  }
  if (error && typeof error === 'object' && 'status' in error && error.status === REQUEST_TOO_LARGE) {
    return { status: REQUEST_TOO_LARGE, body: { code: 'request_too_large', message: 'The JSON request exceeds the server limit.' } }
  }
  return { status: 500, body: { code: 'internal_error', message: 'The server could not complete the request.' } }
}

@Catch()
export class ServerErrorFilter implements ExceptionFilter {
  catch(error: unknown, host: ArgumentsHost): void {
    const response = errorResponse(error)
    host.switchToHttp().getResponse<Response>().status(response.status).json(response.body)
  }
}
