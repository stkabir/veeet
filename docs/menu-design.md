# Transport Dock

## Dirección

El menú se inspira en la barra de transporte de un DAW. Es una franja de controles
que siempre está a mano, abajo, y nunca tapa lo que se escucha. La música ocupa el
centro de la pantalla. El menú vive en el borde y sube solo cuando se le pide.

El menú tiene tres niveles de presencia:

1. **Handle**: una píldora de 52 × 28 px abajo al centro, con un engrane y un punto
   del color del preset activo. Se oculta junto con el cursor en pantalla completa.
2. **Dock**: una barra flotante de 64 px de alto y hasta 760 px de ancho. A la
   izquierda están los chips de preset; el activo se expande con su nombre. Al centro
   hay dos ajustes rápidos: Glow y el ajuste principal del preset (Sensibilidad,
   Curva o Radio). A la derecha están la flecha del cajón y el botón de cerrar.
3. **Cajón**: sube 8 px por encima del dock con una animación de 180 ms (opacidad y
   desplazamiento). Tiene pestañas subrayadas (Preset · Luz · Grid · Ventana · Audio)
   y dos columnas cuando el ancho llega a 520 px. Solo el contenido se desplaza; el
   pie con Guardar, Restablecer, Ocultar y Salir queda fijo.
   La pestaña Audio tiene la fuente (Automático o Todo el sistema), la etiqueta
   de lo que se captura y la lista *Apps con audio*: una fila por app, con un
   punto de actividad, su nombre y el título de su ventana. Al pulsar una fila
   se fija esa app. Restablecer vuelve a Automático.

`Esc` cierra un nivel por vez: primero el cajón, luego el dock y, en pantalla
completa, sale de ella.

## Tokens

Los tokens están en `src/ui/theme.rs`.

| Token | Valor | Uso |
|---|---|---|
| `OBSIDIAN` | `rgb(10,12,16)` α 242 | dock y handle |
| `SLATE` | `rgb(14,17,22)` α 246 | cajón, un nivel por encima del dock |
| `WELL` | `rgb(5,7,10)` | rieles, cápsulas y pistas, hundidos |
| `INK` · `INK_SOFT` · `INK_MUTED` · `INK_FAINT` | grises 238 · 195 · 135 · 95 | cuatro niveles de texto |
| bordes | blanco α 12 / 26 | reposo / hover |

**Acento por preset.** Cian `(96,226,255)` onda, ámbar `(255,178,92)` anillo, verde
`(110,235,170)` Alquimia, violeta `(178,140,255)` Ambiente, lima `(150,255,100)` Scope,
blanco hielo `(200,215,255)` Huracán, turquesa `(90,225,225)` Niagara y coral
`(255,140,110)` Windmill. El acento solo aparece en el control
activo: el relleno del riel, la pestaña actual, el chip seleccionado, el interruptor
encendido, Guardar y el hilo superior del cajón.

**Profundidad.** La profundidad se marca solo con bordes capilares y saltos mínimos
de luminosidad. La única sombra es la que separa el dock del visualizador.

**Tipografía.** Outfit para pestañas, microetiquetas y nombres de preset. Instrument
Sans de 11 px para las etiquetas. Geist Mono de 10 px para valores y estado.

**Espaciado.** La base es de 4 px. El dock tiene 12 px de padding y el cajón 16-18 px.
Los radios son de 6 px en controles, 12 px en chips y 18 px en el dock.

## Controles

- **Riel**: 4 px de alto, relleno de acento y pulgar de 12 px con halo en hover. Un
  clic en cualquier punto del riel fija el valor.
- **Cápsula de valor**: `DragValue` en Geist Mono. Se arrastra para el ajuste fino o
  se hace clic para escribir el número.
- **Interruptor**: pista de 30 × 16 px con animación de 120 ms. Reemplaza a las
  casillas.
- **Chips de preset**: los iconos se dibujan con la misma lógica que el visualizador:
  onda, rejilla LED, y radios con contorno.
