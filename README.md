# Rpress

Framework HTTP/1.1 assíncrono em Rust, construído sobre `tokio`. Projetado para ser leve, seguro e pronto para produção.

## Features

- Roteamento baseado em trie (estático, dinâmico, multi-method)
- Middleware (global e por grupo de rotas)
- Request body streaming via `mpsc::channel`
- Compressão automática gzip/brotli
- CORS nativo com builder pattern
- Rate limiting por IP
- Servir arquivos estáticos
- Cookies (parse e Set-Cookie builder)
- Graceful shutdown
- Timeouts configuráveis (leitura e idle)
- Limite de conexões simultâneas
- Headers de segurança automáticos (`X-Content-Type-Options: nosniff`)
- Request ID automático (`X-Request-ID`)

## Quick Start

```rust
use engine::{Rpress, RpressCors, RpressRoutes, RequestPayload, ResponsePayload};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cors = RpressCors::new()
        .set_origins(vec!["*"])
        .set_methods(vec!["GET", "POST", "PUT", "DELETE"])
        .set_headers(vec!["Content-Type", "Authorization"]);

    let mut app = Rpress::new(Some(cors));

    let mut routes = RpressRoutes::new();
    routes.add(":get/hello", |_req: RequestPayload| async move {
        ResponsePayload::text("Hello, Rpress!")
    });

    app.add_route_group(routes);
    app.listen("0.0.0.0:3000").await?;

    Ok(())
}
```

## Roteamento

As rotas usam o formato `:método/caminho`. Segmentos dinâmicos são prefixados com `:`.

### Rotas estáticas

```rust
let mut routes = RpressRoutes::new();

routes.add(":get/api/users", |_req: RequestPayload| async move {
    ResponsePayload::json(&serde_json::json!({"users": []})).unwrap()
});
```

### Rotas com parâmetros dinâmicos

```rust
routes.add(":get/api/users/:id", |req: RequestPayload| async move {
    let id = req.get_param("id").unwrap_or("0");
    ResponsePayload::text(format!("User ID: {}", id))
});
```

### Multi-method no mesmo path

```rust
routes.add(":get/api/resource", |_req: RequestPayload| async move {
    ResponsePayload::text("GET resource")
});

routes.add(":post/api/resource", |_req: RequestPayload| async move {
    ResponsePayload::text("POST resource").with_status(StatusCode::Created)
});

routes.add(":delete/api/resource/:id", |req: RequestPayload| async move {
    let id = req.get_param("id").unwrap_or("?");
    ResponsePayload::text(format!("Deleted {}", id))
});
```

### Métodos HTTP suportados

`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`

## Middleware

### Middleware global

Aplicado a todas as rotas:

```rust
app.use_middleware(|req, next| async move {
    let uri = req.uri().to_string();
    let method = req.method().to_string();

    tracing::info!("--> {} {}", method, uri);
    let start = std::time::Instant::now();

    let result = next(req).await;

    tracing::info!("<-- {} {} ({:?})", method, uri, start.elapsed());
    result
});
```

### Middleware por grupo de rotas

```rust
let mut routes = RpressRoutes::new();

routes.use_middleware(|req, next| async move {
    if req.header("authorization").is_none() {
        return Err(RpressError {
            status: StatusCode::Unauthorized,
            message: "Token required".to_string(),
        });
    }
    next(req).await
});

routes.add(":get/admin/dashboard", |_req: RequestPayload| async move {
    ResponsePayload::text("Admin area")
});
```

## Request

### Acessando dados do request

```rust
routes.add(":post/api/data", |req: RequestPayload| async move {
    // URI e método
    let uri = req.uri();
    let method = req.method();

    // Headers (chaves em lowercase)
    let content_type = req.header("content-type").unwrap_or("unknown");
    let auth = req.header("authorization");

    // Parâmetros de rota
    let id = req.get_param("id");

    // Query string — GET /search?q=rust&page=1
    let query = req.get_query("q").unwrap_or("");
    let page = req.get_query("page").unwrap_or("1");

    // Cookies
    let cookies = req.cookies();
    let session = cookies.get("session_id");

    // Body como string
    let body_text = req.body_str().unwrap_or("invalid utf8");

    // Body como JSON
    let data: serde_json::Value = req.body_json().unwrap();

    ResponsePayload::text("ok")
});
```

### Body Streaming

Para uploads grandes, o Rpress pode transmitir o body em chunks via channel em vez de acumular tudo na memória. O threshold é configurável:

```rust
app.set_stream_threshold(64 * 1024); // streaming para bodies > 64KB
```

#### `collect_body()` — Uso simples (recomendado)

Coleta o body inteiro em um `Vec<u8>`. Funciona tanto para bodies pequenos (já carregados) quanto para streaming:

```rust
routes.add(":post/upload", |mut req: RequestPayload| async move {
    let body = req.collect_body().await;
    ResponsePayload::text(format!("Received {} bytes", body.len()))
});
```

#### `body_stream()` — Processamento chunk por chunk

Para processar dados sob demanda sem acumular tudo na memória:

```rust
routes.add(":post/stream", |mut req: RequestPayload| async move {
    let mut total = 0usize;

    if let Some(mut rx) = req.body_stream() {
        while let Some(chunk) = rx.recv().await {
            // Processar cada chunk individualmente
            total += chunk.len();
        }
    }

    ResponsePayload::text(format!("Processed {} bytes in chunks", total))
});
```

## Response

### Builders disponíveis

```rust
// Texto simples
ResponsePayload::text("Hello world")

// HTML
ResponsePayload::html("<h1>Welcome</h1>")

// JSON
ResponsePayload::json(&serde_json::json!({"status": "ok"})).unwrap()

// Bytes com content-type customizado
ResponsePayload::bytes(vec![0x89, 0x50, 0x4E, 0x47], "image/png")

// Vazio (204 No Content)
ResponsePayload::empty()

// Redirect
ResponsePayload::redirect("/new-location", StatusCode::Found)
```

### Encadeando modificadores

```rust
ResponsePayload::text("data")
    .with_status(StatusCode::Created)
    .with_content_type("application/xml")
    .with_header("X-Custom", "value")
```

### Cookies

```rust
use engine::CookieBuilder;

let cookie = CookieBuilder::new("token", "abc123")
    .path("/")
    .max_age(3600)
    .same_site("Strict")
    .http_only(true)
    .secure(true)
    .domain("example.com");

ResponsePayload::text("logged in")
    .set_cookie(&cookie)
```

Múltiplos `Set-Cookie` são suportados — cada `.set_cookie()` adiciona um header separado.

## CORS

Configuração nativa via builder pattern:

```rust
let cors = RpressCors::new()
    .set_origins(vec!["https://app.example.com", "https://admin.example.com"])
    .set_methods(vec!["GET", "POST", "PUT", "DELETE"])
    .set_headers(vec!["Content-Type", "Authorization", "X-Custom-Header"])
    .set_expose_headers(vec!["X-Request-ID"])
    .set_max_age(3600)
    .set_credentials(true);

let mut app = Rpress::new(Some(cors));
```

Sem CORS:

```rust
let mut app = Rpress::new(None);
```

Headers automáticos: `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers`, `Vary: Origin`. Preflight `OPTIONS` é tratado automaticamente.

## Compressão

Gzip e Brotli com negociação automática via `Accept-Encoding`:

```rust
app.enable_compression(true);
```

Comportamento:
- Brotli é preferido quando `Accept-Encoding: br` está presente
- Gzip é usado quando `Accept-Encoding: gzip` está presente
- Bodies menores que 256 bytes não são comprimidos
- Tipos já comprimidos (image/*, video/*, audio/*, zip, gzip) são ignorados
- SVG é comprimido normalmente
- `Content-Encoding` e `Vary: Accept-Encoding` são adicionados automaticamente

## Rate Limiting

Limitar requisições por IP usando token bucket:

```rust
app.set_rate_limit(100, 60); // 100 requisições por 60 segundos
```

Quando o limite é excedido, retorna `429 Too Many Requests`. Entradas expiradas são limpas automaticamente quando o store excede 10.000 registros.

## Arquivos Estáticos

```rust
app.serve_static("/assets", "./public");
app.serve_static("/uploads", "/var/data/uploads");
```

- Content-Type é detectado pela extensão do arquivo
- Path traversal é prevenido com `canonicalize()`
- Suporta: HTML, CSS, JS, JSON, imagens (PNG, JPG, GIF, SVG, WebP, ICO), fontes (WOFF, WOFF2, TTF), PDF, XML, vídeos (MP4, WebM)

## Configuração Completa

```rust
use std::time::Duration;

let mut app = Rpress::new(Some(cors));

// Capacidade do buffer de leitura (default: 40KB)
app.set_buffer_capacity(1024 * 1024);

// Timeout de leitura por request (default: 30s)
app.set_read_timeout(Duration::from_secs(30));

// Timeout de idle entre requests keep-alive (default: 60s)
app.set_idle_timeout(Duration::from_secs(120));

// Máximo de conexões simultâneas (default: 1024)
app.set_max_connections(2048);

// Rate limiting
app.set_rate_limit(100, 60);

// Body streaming threshold (default: 64KB)
app.set_stream_threshold(64 * 1024);

// Compressão gzip/brotli (default: desabilitado)
app.enable_compression(true);

// Arquivos estáticos
app.serve_static("/assets", "./public");

// Rotas e middleware
app.use_middleware(|req, next| async move { next(req).await });
app.add_route_group(routes);

// Iniciar servidor
app.listen("0.0.0.0:3000").await?;
```

## Controllers com `handler!` macro

Para organizar handlers em structs com `Arc`:

```rust
use engine::handler;

pub struct UserController;

impl UserController {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    async fn get_user(&self, req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let id = req.get_param("id").ok_or_else(|| RpressError {
            status: StatusCode::BadRequest,
            message: "Missing id".to_string(),
        })?;

        Ok(ResponsePayload::json(&serde_json::json!({
            "id": id,
            "name": "Guilherme"
        }))?)
    }

    async fn create_user(&self, mut req: RequestPayload) -> Result<ResponsePayload, RpressError> {
        let body = req.collect_body().await;
        let data: serde_json::Value = serde_json::from_slice(&body)?;

        Ok(ResponsePayload::json(&serde_json::json!({
            "created": true,
            "name": data["name"]
        }))?.with_status(StatusCode::Created))
    }
}

pub fn get_user_routes() -> RpressRoutes {
    let controller = UserController::new();
    let mut routes = RpressRoutes::new();

    routes.add(":get/users/:id", handler!(controller, get_user));
    routes.add(":post/users", handler!(controller, create_user));

    routes
}
```

## Erros Customizados

Implemente `RpressErrorExt` para retornar erros com status codes customizados:

```rust
use engine::{RpressErrorExt, StatusCode};

struct NotFoundError {
    resource: String,
}

impl RpressErrorExt for NotFoundError {
    fn into_rpress_error(self) -> (StatusCode, String) {
        (StatusCode::NotFound, format!("{} not found", self.resource))
    }
}

routes.add(":get/items/:id", |req: RequestPayload| async move {
    let id = req.get_param("id").unwrap_or("0");
    if id == "0" {
        return Err(NotFoundError { resource: "Item".into() });
    }
    Ok(ResponsePayload::text(format!("Item {}", id)))
});
```

Handlers podem retornar:
- `ResponsePayload` (200 implícito)
- `Result<ResponsePayload, RpressError>`
- `Result<ResponsePayload, E>` onde `E: RpressErrorExt`
- Qualquer `E: RpressErrorExt` diretamente (erro sem Result)
- `()` (202 Accepted sem body)

## Headers de Segurança

Aplicados automaticamente a todas as responses:

| Header | Valor |
|--------|-------|
| `X-Content-Type-Options` | `nosniff` |
| `X-Request-ID` | UUID v4 único por request |
| `Server` | `Rpress/1.0` |
| `Connection` | `keep-alive` |

## Graceful Shutdown

O servidor responde a `SIGINT` (Ctrl+C):

1. Para de aceitar novas conexões
2. Aguarda conexões ativas finalizarem
3. Encerra limpo

## Limites de Segurança

| Recurso | Limite |
|---------|--------|
| Request line | 8 KB |
| Headers (tamanho) | 8 KB |
| Headers (quantidade) | 100 |
| Body (Content-Length) | 10 MB |
| Chunk individual | 1 MB |
| Buffer de conexão | Configurável (default 40 KB) |

## Licença

MIT
