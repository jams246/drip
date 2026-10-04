# syntax=docker/dockerfile:1
FROM node:24-alpine3.24 AS build
WORKDIR /app

COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --ignore-scripts --no-audit --no-fund

COPY tsconfig.server.json ./
COPY src/server ./src/server
COPY src/protocol ./src/protocol
RUN --mount=type=cache,target=/root/.npm \
    npm run build:server -- --sourceMap false \
    && npm pkg delete dependencies.react dependencies.react-dom \
    && npm prune --omit=dev --ignore-scripts --no-audit --no-fund

FROM alpine:3.24 AS runtime
ENV NODE_ENV=production DRIP_DATA_DIR=/data DRIP_PORT=8000
WORKDIR /app

# The server requires util-linux flock, including its --no-fork option.
RUN apk add --no-cache libstdc++ flock \
    && addgroup -g 1000 node \
    && adduser -D -u 1000 -G node node \
    && mkdir /data \
    && chown node:node /data \
    && chmod 0700 /data

COPY --from=build /usr/local/bin/node /usr/local/bin/node
COPY --from=build /app/package.json ./
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/dist ./dist

USER node
EXPOSE 8000
ENTRYPOINT ["node", "dist/server/bootstrap/cli.js"]
CMD ["serve"]
