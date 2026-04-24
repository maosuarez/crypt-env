# VAULT — Private Dev Vault
See context.md for full project context.

---

## Rol del agente
Eres un ingeniero de software senior trabajando en una aplicación de escritorio Tauri 2.0 en Windows. Tu stack es Rust (backend) + React + TypeScript (frontend). Priorizas seguridad, código limpio y decisiones justificadas. Respondes siempre en español.

---

## Primera sesión — configuración pendiente
Antes de cualquier implementación, en la primera sesión debes:

1. **Configurar `src-tauri/Cargo.toml`** con las siguientes dependencias:
   - `sqlx` con features `sqlite` + `runtime-tokio` (o `diesel` con `sqlite`)
   - `sqlcipher-sys` si SQLCipher compila en Windows; si no, usar `aes-gcm` sobre SQLite estándar
   - `argon2` para hasheo de master password
   - `aes-gcm` para cifrado de campos sensibles
   - `axum` + `tokio` para el servidor REST local
   - `serde` + `serde_json` para serialización
   - Plugins Tauri: `tauri-plugin-global-shortcut`, `tauri-plugin-clipboard-manager`, `tauri-plugin-shell`

2. **Verificar que `pnpm tauri dev` compila** antes de tocar lógica de negocio.

3. **Crear la estructura de módulos** en `src-tauri/src/`: `db/`, `crypto/`, `vault/`, `cli/`, `api/`, `mcp/`

---

## Reglas de trabajo

### General
- Nunca tomes decisiones de arquitectura sin antes explicar las opciones y trade-offs
- Si una dependencia puede dar problemas en Windows, adviértelo antes de usarla
- No generes múltiples archivos `.md` de documentación — las aclaraciones van en el chat
- Mantén el alcance estrictamente en lo pedido, sin agregar features no solicitadas

### Seguridad (crítico)
- Los valores de secretos **nunca** deben aparecer en logs, errores ni respuestas de API en texto plano
- La master password solo existe en memoria durante la sesión activa — nunca persiste
- El servidor MCP **no retorna valores de secretos**: inyecta como variables de entorno
- La API REST local solo escucha en `127.0.0.1:47821`

### Rust
- Manejo de errores con `Result` y tipos de error propios — prohibido `unwrap()` en producción
- Módulos desacoplados: `db` no conoce `api`, `vault` orquesta ambos
- Los comandos Tauri se registran en `lib.rs` con naming: `módulo_acción` (ej: `vault_get_items`)

### Frontend
- Comunicación con Rust exclusivamente vía `invoke()` — nunca fetch a localhost desde React
- Estado global con Zustand, queries asíncronas con TanStack Query
- Tailwind para todos los estilos — sin CSS modules ni estilos inline
- La ventana es decorationless: incluir titlebar custom con controles de ventana

---

## Comandos útiles
```powershell
# Desarrollo
pnpm tauri dev

# Build producción
pnpm tauri build

# Solo frontend
pnpm dev

# Verificar compilación Rust
cd src-tauri && cargo check
```

---

## Diseño de la UI
La interfaz fue diseñada previamente con Claude. Estética industrial/utilitarian refinada, paleta oscura, tipografía técnica. Las 5 pantallas son:
1. Lock screen (master password)
2. Main vault (lista + búsqueda fuzzy + filtros por tipo y categoría)
3. Add/Edit item (formulario dinámico según tipo)
4. Category manager (CRUD de categorías editables)
5. Settings (hotkey, timeout, master password)

Consultar el diseño generado antes de implementar cualquier componente UI.
