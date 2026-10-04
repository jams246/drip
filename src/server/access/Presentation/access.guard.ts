import { Inject, Injectable } from '@nestjs/common'
import type { CanActivate, ExecutionContext } from '@nestjs/common'
import { Reflector } from '@nestjs/core'
import type { Request } from 'express'
import { AccessApplication } from '../Application/access.js'

@Injectable()
export class AccessGuard implements CanActivate {
  constructor(
    @Inject(AccessApplication) private readonly access: AccessApplication,
    @Inject(Reflector) private readonly reflector: Reflector
  ) {}

  canActivate(context: ExecutionContext): boolean {
    if (this.reflector.get<boolean>('access.public', context.getHandler())) return true
    const request = context.switchToHttp().getRequest<Request & { deviceId: string }>()
    request.deviceId = this.access.authenticate(request.headers.authorization).deviceId
    return true
  }
}
