# Site + démo (même build que Vercel : pnpm demo:build → site/), servi par static-web-server
FROM node:24-alpine AS build
WORKDIR /app
RUN corepack enable && corepack prepare pnpm@10 --activate
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
RUN pnpm demo:build

FROM joseluisq/static-web-server:2-alpine
COPY --from=build /app/site /public
COPY sws.toml /etc/sws.toml
ENV SERVER_CONFIG_FILE=/etc/sws.toml
EXPOSE 80
