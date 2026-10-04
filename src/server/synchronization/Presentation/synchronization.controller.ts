import { Body, Controller, Delete, Get, HttpCode, Param, Post, Put, Query, Req } from '@nestjs/common'
import type { IncomingMessage } from 'node:http'
import { MAX_REGION_BYTES } from '../../../protocol/sync.js'
import { SynchronizationApplication } from '../Application/synchronization.js'
import { SyncError } from '../Domain/errors.js'

type DeviceRequest = IncomingMessage & { deviceId: string }
const NO_CONTENT = 204
const ACCEPTED = 202

@Controller('v1')
export class SynchronizationController {
  constructor(private readonly application: SynchronizationApplication) {}

  @Post('roots')
  registerRoot(@Req() request: DeviceRequest, @Body() body: unknown) {
    return this.application.registerRoot(request.deviceId, body)
  }

  @Get('roots/:rootId/heads')
  heads(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Query() query: Record<string, string>) {
    return this.application.listHeads(request.deviceId, rootId, {
      revision: query.revision === undefined ? undefined : Number(query.revision),
      after: query.after,
      limit: query.limit === undefined ? undefined : Number(query.limit)
    })
  }

  @Post('roots/:rootId/operations')
  offer(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Body() body: unknown) {
    return this.application.offer(request.deviceId, rootId, body)
  }

  @Post('roots/:rootId/operations/:operationId/regions')
  regions(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Param('operationId') operationId: string, @Body() body: unknown) {
    return this.application.addRegions(request.deviceId, rootId, operationId, body)
  }

  @Post('roots/:rootId/operations/:operationId/regions/complete')
  @HttpCode(ACCEPTED)
  completeRegions(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Param('operationId') operationId: string) {
    return this.application.completeRegions(request.deviceId, rootId, operationId)
  }

  @Get('roots/:rootId/operations/:operationId/regions')
  listRegions(
    @Req() request: DeviceRequest,
    @Param('rootId') rootId: string,
    @Param('operationId') operationId: string,
    @Query() query: Record<string, string>
  ) {
    return this.application.listRegions(request.deviceId, rootId, operationId, {
      after: query.after === undefined ? undefined : Number(query.after),
      limit: query.limit === undefined ? undefined : Number(query.limit)
    })
  }

  @Put('roots/:rootId/operations/:operationId/regions/:offset')
  @HttpCode(NO_CONTENT)
  async upload(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Param('operationId') operationId: string, @Param('offset') offset: string) {
    await this.application.uploadRegion(request.deviceId, rootId, operationId, Number(offset), await readRegion(request))
  }

  @Post('roots/:rootId/operations/:operationId/commit')
  @HttpCode(ACCEPTED)
  commit(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Param('operationId') operationId: string) {
    return this.application.commit(request.deviceId, rootId, operationId)
  }

  @Get('roots/:rootId/operations/:operationId')
  status(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Param('operationId') operationId: string) {
    return this.application.status(request.deviceId, rootId, operationId)
  }

  @Delete('roots/:rootId/operations/:operationId')
  abort(@Req() request: DeviceRequest, @Param('rootId') rootId: string, @Param('operationId') operationId: string) {
    return this.application.abort(request.deviceId, rootId, operationId)
  }
}

async function readRegion(request: IncomingMessage): Promise<Uint8Array> {
  if (request.headers['content-type']?.split(';')[0] !== 'application/octet-stream')
    throw new SyncError('invalid_request', 'Region uploads require application/octet-stream.')
  if (Number(request.headers['content-length']) > MAX_REGION_BYTES) throw new SyncError('invalid_request', 'Region upload exceeds size limit.')
  const parts: Uint8Array[] = []
  let size = 0
  for await (const part of request) {
    if (!(part instanceof Uint8Array)) throw new SyncError('invalid_request', 'Invalid region stream.')
    size += part.length
    if (size > MAX_REGION_BYTES) throw new SyncError('invalid_request', 'Region upload exceeds size limit.')
    parts.push(part)
  }
  return Buffer.concat(parts, size)
}
