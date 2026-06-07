# Roadmap / Lista de Deseos

## 1. Configuración automática del PATH
Evaluar la posibilidad de que el instalador agregue automáticamente la carpeta de binarios al `PATH` del sistema.

**Objetivos:**
- Evitar configuraciones manuales.
- Garantizar que los comandos estén disponibles desde cualquier terminal.
- Confirmar viabilidad y comportamiento esperado en Windows, Linux y macOS.

---

## 2. Extensión para navegadores
Explorar el desarrollo de una extensión compatible con navegadores modernos.

**Opciones iniciales:**
- Firefox.
- Google Chrome.
- Navegadores basados en Chromium.

**Objetivos:**
- Integrar funcionalidades clave directamente desde el navegador.
- Mejorar accesibilidad y adopción.

---

## 3. Comandos Funcionan sin app activa
Desacoplar la aplicacion de escritorio de las funcioanlidades TUI, CLI, MCP

**Objetivos**
- Estuve pensando en que no debo tener la app activa para hacer uso de otras funcionalidades si se cuenta con el token de acceso, o la contrasena en casos como los de la tui o la cli
- Podemos hacer uso de las herramientas incluso sin habilidar la aplicacion de escritorio que siempre es un paso adicional
- Asegurar la seguirdad verificando tokesns, o posibles contrasenas pero ahora son independientes

 