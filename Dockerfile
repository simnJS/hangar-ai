# The site and its demo (pnpm demo:build fills site/), served by static-web-server behind
# Cloudflare at https://hangar.simnjs.fr. See site/README.md.
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
