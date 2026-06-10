# Etapa 1: Construcción (Usamos musl para enlace estático puro)
FROM rust:1.77-alpine AS builder

# Instalar musl-dev para compilación en Alpine
RUN apk add --no-cache musl-dev

WORKDIR /app
COPY Cargo.toml ./
COPY src ./src

# Compilar release optimizado
RUN cargo build --release --target x86_64-unknown-linux-musl

# Etapa 2: Contenedor Scratch (< 5MB final)
FROM scratch

# Copiar certificados raíz (Requeridos por rustls para conexiones HTTPS a Meta)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copiar el binario compilado (Notar que el path incluye el target musl)
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/meta-proxy /meta-proxy

EXPOSE 8080
ENTRYPOINT ["/meta-proxy"]
