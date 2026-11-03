# The site (site/), served by static-web-server behind Cloudflare at
# https://hangar.simnjs.fr. See site/README.md. The page has no build step, so
# the image is the server and the folder as it is.
FROM joseluisq/static-web-server:2-alpine
COPY site /public
COPY sws.toml /etc/sws.toml
ENV SERVER_CONFIG_FILE=/etc/sws.toml
EXPOSE 80
