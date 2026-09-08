# ==============================================================================
# STAGE 1: Dependency Installation
# ==============================================================================
FROM node:20-alpine AS deps
RUN apk add --no-cache libc6-compat
WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm ci

# ==============================================================================
# STAGE 2: Application Build
# ==============================================================================
FROM node:20-alpine AS builder
WORKDIR /app

COPY --from=deps /app/node_modules ./node_modules
COPY . .

# Pass public daemon endpoint to client-side assets during build
ARG NEXT_PUBLIC_RAQIM_DAEMON_URL=http://localhost:8081
ENV NEXT_PUBLIC_RAQIM_DAEMON_URL=$NEXT_PUBLIC_RAQIM_DAEMON_URL
ENV NEXT_TELEMETRY_DISABLED=1

RUN npm run build

# ==============================================================================
# STAGE 3: Production Runtime
# ==============================================================================
FROM node:20-alpine AS runner
WORKDIR /app

ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
ENV PORT=3000
ENV HOSTNAME="0.0.0.0"

RUN addgroup --system --gid 1001 nodejs && \
    adduser --system --uid 1001 nextjs

# Copy built application assets and dependencies
COPY --from=builder /app/public ./public
COPY --from=builder /app/package.json ./package.json
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder --chown=nextjs:nodejs /app/.next ./.next

USER nextjs

EXPOSE 3000

CMD ["npm", "run", "start"]