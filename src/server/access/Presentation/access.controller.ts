import { Body, Controller, Get, Inject, Post, Req, SetMetadata } from '@nestjs/common'
import type { Request } from 'express'
import { AccessApplication } from '../Application/access.js'

@Controller('v1')
export class AccessController {
  constructor(@Inject(AccessApplication) private readonly access: AccessApplication) {}

  @Post('enroll')
  @SetMetadata('access.public', true)
  enroll(@Body() body: unknown) {
    return this.access.enroll(body)
  }

  @Get('device')
  device(@Req() request: Request) {
    return this.access.authenticate(request.headers.authorization)
  }
}
