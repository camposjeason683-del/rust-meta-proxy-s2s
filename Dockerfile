# Etapa 1: Construcción en Alpine (que ya usa musl por defecto)
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app
COPY Cargo.toml ./
COPY src ./src

# Asegurar que el target musl estatico este instalado
RUN rustup target add x86_64-unknown-linux-musl

# Compilar release optimizado con el target especifico (Esto evita romper proc-macros)
RUN cargo build --release --target x86_64-unknown-linux-musl

# Etapa 2: Contenedor Scratch (< 5MB final)
FROM scratch

# Copiar certificados raiz (Requeridos por rustls para conexiones HTTPS a Meta)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copiar el binario compilado (Notar que el path incluye el target musl de nuevo)
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/meta-proxy /meta-proxy

EXPOSE 8080
ENTRYPOINT ["/meta-proxy"]
