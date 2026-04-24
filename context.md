# tauri-private-vault

## Descripción
Bóveda personal de productividad para desarrolladores. Centraliza credenciales, API keys, tokens, contraseñas, links, comandos y notas en una app de escritorio local accesible por hotkey (Ctrl+Alt+Z). Los secretos se almacenan cifrados localmente. Incluye CLI, API REST local y servidor MCP para integración con herramientas externas.

## Stack
- **Frontend**: React 18 + TypeScript + Vite + Tailwind CSS + Framer Motion
- **Backend (Rust)**: Tauri 2.0, Axum (REST local), Tokio
- **Base de datos**: SQLite con `libsqlite3-sys` bundled (no SQLCipher; ver Decisión #1)
- **Cifrado**: AES-256-GCM para campos sensibles, Argon2id para master password, `subtle::ConstantTimeEq` para comparaciones timing-safe
- **CLI**: Interfaz de terminal para gestión de ítems sin abrir la GUI (binario `vault`)
- **MCP**: Servidor Model Context Protocol para consulta segura de secretos (binario `vault-mcp`)
- **API REST**: Axum en `127.0.0.1:47821` con autenticación dual (session token + MCP token)
- **OS objetivo**: Windows (desarrollo), multiplataforma en el futuro
- **Package manager**: pnpm

## Arquitectura
```
tauri-private-vault/
├── src/                          # Frontend React
│   ├── components/               # Componentes UI por pantalla
│   ├── store/                    # Estado global con Zustand
│   ├── hooks/                    # Custom hooks para invoke() de Tauri
│   └── types/                    # Tipos TypeScript compartidos
├── src-tauri/
│   ├── src/
│   │   ├── main.rs               # Entrypoint Tauri (inicializa lib)
│   │   ├── lib.rs                # Registro de comandos Tauri (10+), setup de AppState
│   │   ├── db/mod.rs             # Pool SQLite, tablas, CRUD items/categories/settings
│   │   ├── crypto/mod.rs         # Argon2id KDF + AES-256-GCM encrypt/decrypt
│   │   ├── vault/mod.rs          # VaultState, 10+ comandos Tauri, change_password, wipe
│   │   ├── api/mod.rs            # Servidor Axum en 127.0.0.1:47821, auth dual token
│   │   ├── cli/mod.rs            # Módulo CLI (stub)
│   │   ├── mcp/mod.rs            # Módulo MCP (stub)
│   │   └── bin/
│   │       ├── vault.rs          # CLI standalone (clap), conecta vía HTTP a API
│   │       └── vault-mcp.rs      # Servidor MCP JSON-RPC 2.0 sobre stdio
│   ├── Cargo.toml                # Dependencias Rust
│   └── tauri.conf.json           # Config de ventana, permisos, hotkey
```

**Comunicación**:
- Frontend → `invoke()` de Tauri → comandos registrados en Rust
- CLI (`vault`) → HTTP REST a `127.0.0.1:47821` con session/MCP token
- MCP (`vault-mcp`) → HTTP REST a `127.0.0.1:47821` con MCP token

## Tipos de ítem en la bóveda
1. **Secret / API Key**: nombre, valor cifrado, categoría, notas. Export como `.env` / `export` / `$env:`
2. **Credential**: nombre del sitio, URL, usuario, contraseña cifrada, notas
3. **Link**: título, URL, descripción, categoría
4. **Command**: nombre, comando, descripción, shell target (bash/zsh/PowerShell/cmd), placeholders `{{VAR}}`
5. **Note**: título, contenido libre, categoría

## Seguridad — decisiones tomadas
- **Master password** derivada con Argon2id (m=65536, t=3, p=4), nunca almacenada en texto plano
- **Valores sensibles** cifrados con AES-256-GCM antes de escribir a SQLite
- **Comparaciones timing-safe** usando `subtle::ConstantTimeEq` para tokens y verify_token
- **Clave en memoria** almacenada en `Zeroizing<[u8;32]>` que sobrescribe automáticamente al hacer Drop
- **MCP no expone valores directamente**: inyecta secretos como variables de entorno en el proceso del cliente, sin retornarlos como texto
- **API REST local** escucha solo en `127.0.0.1:47821`, nunca en interfaces externas
- **Autenticación dual**: session tokens (con expiración) + MCP token (estático, almacenado en `%APPDATA%`)
- **Ventana se bloquea** automáticamente tras timeout configurable, poniendo `VaultState.key = None`

## Convenciones
- Idioma del código: inglés (variables, funciones, comentarios)
- Idioma de respuestas del agente: español
- Naming Rust: snake_case. Naming React/TS: camelCase, PascalCase para componentes
- Los comandos Tauri (`invoke`) se nombran con prefijo del módulo: `vault_get_items`, `crypto_unlock`, etc.
- No usar `unwrap()` en producción — manejar errores con `Result` y tipos de error propios
- Tailwind para estilos, sin CSS modules ni styled-components

## Restricciones
- Las dependencias de `src-tauri/Cargo.toml` están **pendientes**: se configuran en la primera sesión de Claude Code
- No implementar sincronización en la nube en esta versión
- No asumir que SQLCipher compila sin fricción en Windows — tener plan B (AES-GCM sobre SQLite estándar)
- La ventana es **decorationless** (sin titlebar del SO), con titlebar custom en React
- No guardar la master password en memoria más tiempo del necesario para desbloquear
- El MCP es de solo lectura — no permite crear ni modificar ítems

## Contexto de negocio
Usuario desarrollador que actualmente comparte credenciales de forma insegura por WhatsApp. Necesita acceso rápido (hotkey), facilidad para copiar al clipboard, y poder usar secretos como variables de entorno sin exponerlos visualmente. Uso estrictamente personal y local.

---

## Decisiones de implementación

### 1. SQLite + AES-GCM en lugar de SQLCipher (Plan B)
**Contexto**: SQLCipher requiere OpenSSL/vcpkg con configuración compleja en Windows, generando errores de enlace (linking) durante la compilación.

**Decisión**: Adoptar Plan B: SQLite estándar con `libsqlite3-sys` bundled + cifrado AES-256-GCM a nivel de aplicación.

**Rationale**: 
- Evita compilación de OpenSSL en Windows (fricción alta, mantenimiento costoso)
- Todos los campos sensibles (`data` en `items`, `data` en `categories`) se cifran antes de escribir a BD
- La DB en disco no está cifrada a nivel de archivo, pero los datos más sensibles están protegidos por AES-256-GCM
- Permite integración futura con bases de datos de mayor escala

**Consecuencias**:
- Si el archivo `vault.db` se accede directamente sin ejecutar la aplicación, los datos permanecen cifrados a nivel de campos
- Se asume control físico del equipo (Windows local, usuario único) — no es defensa contra ataques directos a memoria
- La clave AES derivada existe solo en memoria durante la sesión activa

---

### 2. Estructura de módulos Rust desacoplada
**Contexto**: Necesidad de separar responsabilidades entre crypto, persistencia, API y lógica de negocio.

**Decisión**: 
- `crypto/mod.rs`: Argon2id KDF, AES-256-GCM encrypt/decrypt, manejo de claves en `Zeroizing`
- `db/mod.rs`: Pool SQLite, DDL de tablas, CRUD de ítems/categorías/settings (no conoce `api`, `vault`)
- `vault/mod.rs`: `VaultState` (orquestador), 10+ comandos Tauri, operaciones de unlock/lock
- `api/mod.rs`: Servidor Axum REST, endpoints, autenticación con tokens

**Rationale**: Cada módulo tiene una responsabilidad clara. `vault` orquesta entre `crypto` y `db` sin que estos se conozcan mutuamente.

**Consecuencias**: 
- La API REST (`api/mod.rs`) también usa los mismos módulos subyacentes
- CLI y MCP hablan con el backend vía HTTP REST; no tienen linkage directo con Rust

---

### 3. Almacenamiento de MCP token en archivo
**Contexto**: MCP server necesita token para autenticar llamadas a la API REST; requiere persistencia entre sesiones (sin expiración).

**Decisión**: 
- Token MCP: 32 bytes generado aleatoriamente con `rand::thread_rng()`, guardado en `vault_meta.mcp_token` (DB)
- Copia redundante en `%APPDATA%\com.maosuarez.vault\mcp_token` (archivo plaintext)
- Se genera una única vez con `vault_generate_mcp_token` cuando se inicia MCP por primera vez
- Sin expiración, válido mientras el vault esté desbloqueado

**Rationale**: 
- Permite al MCP server leer su token sin necesidad de desbloquear interactivamente
- Archivo en `%APPDATA%` evita que deba ser leído desde DB cada vez
- La verificación del token en API REST usa `subtle::ConstantTimeEq`

**Consecuencias**:
- El archivo `mcp_token` en `%APPDATA%` necesita permisos restrictivos (idealmente 0600, en Windows: solo propietario)
- Si se compromete ese archivo, cualquiera puede hacer llamadas al MCP

---

### 4. Esquema de base de datos (SQLite en `%APPDATA%`)
**Contexto**: Necesidad de almacenar ítems cifrados, categorías, metadatos de crypto y settings.

**Decisión**: 4 tablas en `vault.db` ubicada en `%APPDATA%\com.maosuarez.vault\vault.db`:

```sql
CREATE TABLE vault_meta (
    id    INTEGER PRIMARY KEY,
    key   TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
-- Contiene: kdf_salt (hex, 32 bytes), verify_token (hex, cifrado con AES-GCM)

CREATE TABLE items (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    item_type  TEXT NOT NULL,  -- 'secret', 'credential', 'link', 'command', 'note'
    data       TEXT NOT NULL,  -- JSON cifrado con AES-GCM
    created_at TEXT NOT NULL,  -- ISO 8601
    updated_at TEXT NOT NULL   -- ISO 8601
);

CREATE TABLE categories (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    data  TEXT NOT NULL  -- JSON array cifrado
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Keys: auto_lock_minutes, hotkey, mcp_token
```

**Rationale**:
- `vault_meta`: almacena salt (público) y verify_token (privado, cifrado) para derivación de clave
- `items.data`: JSON serializado cifrado (evita columnas individuales)
- `categories.data`: array JSON único, cifrado (CRUD simple)
- `settings`: plaintext (no contiene secretos, solo configuración de usuario)

**Consecuencias**:
- La tabla `items` crece indefinidamente; indexación por `id` y `item_type` recomendada para búsquedas futuras
- JSON cifrado requiere deserialización post-decryption en la aplicación

---

### 5. Flujo de unlock y gestión de clave en sesión
**Contexto**: La clave AES debe existir solo en memoria durante la sesión activa; debe destruirse al bloquear.

**Decisión**:

1. **Primera inicialización** (`init_vault_crypto`):
   - Genera `salt` de 32 bytes con `rand::thread_rng()`
   - Deriva clave AES con Argon2id(m=65536, t=3, p=4) desde password + salt
   - Cifra `b"vault_ok_v1"` como `verify_token` con AES-256-GCM
   - Guarda `salt` y `verify_token` en `vault_meta`
   - Guarda clave en `VaultState.key` como `Zeroizing<[u8;32]>`

2. **Unlock** (`unlock_vault_crypto`):
   - Lee `salt` y `verify_token` de `vault_meta`
   - Re-deriva clave con Argon2id
   - Intenta descifrar `verify_token` → si OK, password correcto
   - Guarda clave en `VaultState.key`
   - Genera session token (32 bytes hex)
   - Retorna token al cliente

3. **Lock**:
   - Pone `VaultState.key = None`
   - El `Zeroizing` automáticamente sobrescribe los 32 bytes al hacer Drop

**Rationale**:
- `Zeroizing` es obligatorio para evitar que la clave persista en heap entre sesiones
- Argon2id con parámetros altos (m=65536) hace brute-force muy costoso
- `verify_token` permite detectar password incorrecto sin necesidad de descifrar todos los ítems

**Consecuencias**:
- El tiempo de unlock es ~200-500ms (por diseño, Argon2id es lento)
- Si el proceso se mata abruptamente, la clave puede no sobrescribirse (defensa contra ataques de DMA no es posible en Windows user-mode)

---

### 6. Autenticación REST: session token vs MCP token
**Contexto**: API REST debe autenticar solicitudes; session tokens expiran, MCP token es persistente.

**Decisión**:

- **Session token**: 32 bytes hex, generado en `/unlock`, válido por `auto_lock_minutes` (Instant + Duration en servidor)
  - Header: `X-Vault-Token: <hex32>`
  - Expira automáticamente
  - Usado por CLI y frontend (vía Tauri `invoke`)

- **MCP token**: 32 bytes hex, generado una vez, sin expiración
  - Header: `X-Vault-Token: <hex32>` (mismo header)
  - Verificación en tiempo-constante con `subtle::ConstantTimeEq`
  - Usado únicamente por servidor MCP
  - Permite MCP funcionar sin que el vault esté "desbloqueado" interactivamente

**Rationale**: 
- Dos canales separados: session (efímera, UI) vs MCP (persistente, backend)
- MCP puede funcionar sin interfaz gráfica
- Expiración previene reutilización de tokens exfiltrados

**Consecuencias**:
- Servidor debe mantener mapa `HashMap<String, Instant>` de tokens activos
- Limpieza periódica de tokens expirados recomendada

---

### 7. Endpoints REST implementados
**Contexto**: API REST en `127.0.0.1:47821` como interfaz unificada para CLI, MCP y Tauri.

**Decisión**: Implementar endpoints RESTful con autenticación dual:

| Método | Ruta | Auth | Descripción |
|--------|------|------|-------------|
| POST | `/unlock` | - | Valida password, retorna session token |
| GET | `/items` | token | Lista ítems (sin campos sensibles) |
| POST | `/items` | token | Crea ítem nuevo |
| GET | `/items/:id` | token | Obtiene ítem (sin valores cifrados) |
| PUT | `/items/:id` | token | Actualiza ítem |
| DELETE | `/items/:id` | token | Elimina ítem |
| POST | `/items/:id/reveal` | token | Descifra y retorna valor sensible (único endpoint que lo hace) |
| GET | `/categories` | token | Lista categorías |
| POST | `/categories` | token | Crea/actualiza categorías |
| GET | `/settings` | token | Retorna settings (no secretos) |
| PUT | `/settings` | token | Actualiza settings |
| GET | `/commands` | token | Lista comandos disponibles (solo lectura MCP) |

**Rationale**:
- `/unlock` sin token (puerta de entrada)
- `/items/:id/reveal` es el único endpoint que retorna secretos en plaintext (justificable porque requiere session token válido)
- Responses nunca incluyen valores cifrados en plaintext (solo JSON de metadatos)

**Consecuencias**:
- CLI debe hacer 2 llamadas: `/unlock` + luego requests autenticadas
- MCP hace un `/unlock` inicial o reutiliza MCP token directamente
- Auditoría de llamadas a `/items/:id/reveal` recomendada (puede loguear accesos)

---

### 8. CLI (`vault` binario)
**Contexto**: Herramienta standalone para gestión sin GUI, escrita en Rust + clap, conecta vía HTTP REST.

**Decisión**: Binario `src-tauri/src/bin/vault.rs` que:
- Usa `clap` para parsing de argumentos
- Conecta vía HTTP a `127.0.0.1:47821` (si el vault GUI está corriendo) o inicia servidor API localmente
- Almacena session token en `%APPDATA%\com.maosuarez.vault\cli_session_token` (con expiración)
- Soporta comandos:
  - `vault unlock` — solicita master password, guarda token
  - `vault lock` — invalida sesión
  - `vault list [--type TYPE]` — lista ítems sin secretos
  - `vault get <name>` — muestra metadata
  - `vault set <name>` — copia valor al clipboard (requiere `/reveal`)
  - `vault fill <name>` — exporta como `export VAR=value` (para `eval`)
  - `vault cmd <name>` — ejecuta comando guardado
  - `vault add` — asistente interactivo

**Rationale**: CLI desacoplado del servidor REST permite control independiente; almacenamiento de token evita re-autenticar.

**Consecuencias**:
- Token en archivo `cli_session_token` necesita permisos restrictivos (0600)
- Si servidor API está inactivo, CLI debe poder levantarlo (posible futura característica)

---

### 9. MCP server (`vault-mcp` binario)
**Contexto**: Servidor Model Context Protocol para integración con agentes IA, comunicación vía JSON-RPC 2.0 sobre stdio.

**Decisión**: Binario `src-tauri/src/bin/vault-mcp.rs` que:
- Lee MCP token de `%APPDATA%\com.maosuarez.vault\mcp_token`
- Conecta vía HTTP REST a `127.0.0.1:47821`
- Implementa tools JSON-RPC:
  - `vault_list_items` — lista sin secretos
  - `vault_get_item` — obtiene por ID/nombre
  - `vault_generate_env` — escribe `.env` en `%TEMP%` (RAII pendiente)
  - `vault_inject_env` — set_var en proceso actual (validación `[A-Z0-9_]+`)
  - `vault_add_item` — crea ítem
  - `vault_update_settings` — modifica settings
  - `vault_list_commands` — lista comandos
  - `vault_run_command` — ejecuta comando shell

**Rationale**:
- Protocolo estándar MCP permite integración con cualquier cliente compatible
- No retorna secretos en plaintext, solo inyecta como variables de entorno
- Token MCP persistente permite funcionar sin interfaz de unlock explícita

**Consecuencias**:
- Si `mcp_token` se compromete, MCP puede ser accedido remotamente (si se escucha en red, lo cual está fuera de alcance actual)
- `vault_inject_env` requiere validación estricta de nombres (prevenir inyección)

---

### 10. Ubicación de archivos en Windows
**Contexto**: Necesidad de almacenar DB, tokens, configuración de forma persistente y segura.

**Decisión**: Usar `%APPDATA%\com.maosuarez.vault\` como directorio base:

```
%APPDATA%\com.maosuarez.vault\
├── vault.db                    # BD SQLite (cifrado AES-GCM a nivel de campos)
├── mcp_token                   # Token MCP (plaintext, permisos 0600)
├── cli_session_token           # Session token CLI (plaintext, permisos 0600)
└── logs/                        # (futuro) Auditoría de accesos
```

**Rationale**: 
- `%APPDATA%` es estándar para datos de usuario en Windows (roameable en dominio)
- Subdirectorio `com.maosuarez.vault` evita conflictos con otras aplicaciones
- Tokens en archivo en lugar de memory-only facilita acceso por CLI/MCP sin servidor GUI

**Consecuencias**:
- Si cuenta de usuario se compromete, los tokens también
- Encriptado a nivel de SO (NTFS EFS) opcional pero no implementado

---

## Estado de seguridad (post-revisión 2026-04-24)

Se realizó una **revisión de seguridad comprehensiva** que identificó **19 hallazgos** (7 ALTA, 8 MEDIA, 4 BAJA). 

**Hallazgos críticos (ALTA) implementados**:
1. ✅ **Timing-safe token comparison**: Implementado `subtle::ConstantTimeEq` para todas las comparaciones de tokens
2. ✅ **Master password derivation**: Argon2id con parámetros reforzados (m=65536, t=3, p=4)
3. ✅ **Clave en memoria con Zeroizing**: Usar `zeroize` crate para sobrescribir clave al hacer Drop

**Hallazgos críticos (ALTA) pendientes de implementación**:
4. ⏳ **Rate limiting en `/unlock`**: Evitar brute-force de master password
5. ⏳ **Auditoría de accesos a `/items/:id/reveal`**: Loguear quién accede a qué secretos y cuándo
6. ⏳ **Permisos de archivos (mcp_token, cli_session_token)**: Configurar 0600 en creación
7. ⏳ **Cifrado de credenciales en el MCP server**: Mantener tokens en memoria con Zeroizing

**Hallazgos MEDIA implementados**:
- ✅ Gestión de errores sin exposición de paths internos

**Hallazgos MEDIA pendientes**:
- ⏳ HTTPS para API REST local (mkcert)
- ⏳ Validación de entrada en `/items` POST/PUT
- ⏳ Limpieza de `.env` temporal generado por `vault_generate_env` (RAII)
- ⏳ Session timeout no implementado aún (solo estructura)

**Hallazgos BAJA**:
- Documentación de seguridad
- Trazabilidad de cambios (audit log)
- Exportación segura de datos

---

> Este archivo es el contexto principal del proyecto.
> Referenciado desde CLAUDE.md con: `See context.md for full project context.`
