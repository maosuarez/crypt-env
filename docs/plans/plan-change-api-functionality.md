Change:
API
1. Cuando se hace una consulta se debe incluir el nombre de un workspace. y el ambiente en el qeu se esta trabajando.
2. get items traeria los items del ambiente y workspace correspondiente
3. post items lo mismo en el lugar adecuado para ello
4. get commands lo mismo
5. /fill debe tener el workspace y el ambiente. si se especifica un nombre se crea ese archivo especifico. sino se agrega el sufijo del ambiente especifico
6. crear el endpoint example teniendo en cuenta el ambiente se crea para que ese si se peuda commitear. todos los valores vacios o con placeholders preestablecidos.
7. share listen debe especificar el workspace y el ambiente.
8. lo mismo con share connect
9. el post /environment se debe hacer especificando un proyecto
10. post /environments/:id/inject es muy similar al 5.
11. /health no debe devolver el numero de llaves eso cambio. 
12. debemos prestar especial atencion a las notas presentes en @references.md

CLI
1. add debe especificar un proyecto con --project y un ambiente con --env para saber donde guardarlo. en caso de no tenerlo se guarda en el ambiente por defecto del proyecto con el nombre de la carpeta en la que se ejecuta al accion. ~/programas/crypt-env $ crypt-env add ... se agrega al proyecto llamado crypt-env asi no este creado se crea con cosas basicas. o siguiedo la configuracion de un archivo llamado crypt-env.json que debe estar en la base del proyecto
2. fill hace lo inverso al add... asi que si no se establece crea las configuraciones del .json o lo default que es un .env con las variabels que se tienen.
3. las demas acciones al igual que las anteriores se deben cambiar a que dependen de carpetas o de ambientes o de ambas. 
4. categorias y configuraciones generales se salvan.
5. doctor debe revisar que el crypt-env.json si es valido
6. revisar tambien las notas de esta seccion

TUI
Bueno actualizarlo con base a las caracteristicas pasadas. y tambien leer las notas. 

MCP
Debemos tener en cuenta todo lo pasado. de los ambientes. y de que leamos las notas. despeus de eso revisamos todo. lee las notas

las notas estan para que se hagan correcciones sobre lo qeu es posible. asi que lo ideal es que podamos actualizarlas despeus de que todos estos cambios se realicen. actualiza despeus de comprobar que es util tu entrega. @reference.md

