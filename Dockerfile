FROM alpine:latest
RUN apk add --no-cache dumb-init

COPY target/release/galry /usr/local/bin/galry
WORKDIR /pictures

EXPOSE 8080
ENTRYPOINT ["dumb-init", "--"]
CMD [ "/usr/local/bin/galry", "/pictures" ]
