# Etapa 1: Construcción en Alpine (que ya usa musl por defecto)
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app
COPY Cargo.toml ./
COPY src ./src

# Compilamos el binario 100% estático
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN cargo build --release

# Etapa 2: Contenedor Scratch (< 5MB final)
FROM scratch

# Copiar certificados raíz (Requeridos por rustls para conexiones HTTPS a Meta)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copiar el binario estático compilado
COPY --from=builder /app/target/release/meta-proxy /meta-proxy

EXPOSE 8080
ENTRYPOINT ["/meta-proxy"]
