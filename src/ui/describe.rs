//! Descripción corta de cada campo del cajón: qué cambia en la escena.
//! Se busca por motor y etiqueta; si el motor no tiene una propia, se usa la
//! común. Las etiquetas son las mismas que muestran los sliders.

use crate::config::VisualPreset;

/// Texto que explica qué cambia el campo `label` en la escena `engine`.
pub fn describe(engine: VisualPreset, label: &str) -> Option<&'static str> {
    specific(engine, label).or_else(|| common(label))
}

fn specific(engine: VisualPreset, label: &str) -> Option<&'static str> {
    use VisualPreset as P;
    Some(match (engine, label) {
        // Onda
        (P::WavyForm3, "ancho") => "Qué tanto de la pantalla ocupa la onda a lo ancho.",
        (P::WavyForm3, "líneas") => "Cuántas copias de la onda se dibujan encimadas.",
        (P::WavyForm3, "distancia de cámara") => "Acerca o aleja la onda; menos es más zoom.",
        (P::WavyForm3, "variación") => "Cuánto se separan y ondulan las líneas entre sí.",
        (P::WavyForm3, "ritmo de variación") => "Qué tan rápido cambia esa variación.",
        // Anillo
        (P::WaveRing, "anillos") => "Un anillo por rango de frecuencia: graves al centro.",
        (P::WaveRing, "radio") => "Tamaño del anillo exterior; 1 llega al borde.",
        (P::WaveRing, "anillo interior") => "Tamaño del anillo más chico respecto al exterior.",
        (P::WaveRing, "amplitud de la onda") => "Qué tanto se deforma cada anillo con el audio.",
        (P::WaveRing, "detalle") => "Puntos por anillo: más es una curva más suave.",
        (P::WaveRing, "rotación") => "Velocidad de giro de los anillos; negativo al revés.",
        (P::WaveRing, "estela líquida") => "Cuánto dura el rastro que dejan los anillos.",
        (P::WaveRing, "flujo de la estela") => "Qué tanto se mueve y ondula ese rastro.",
        // Alquimia
        (P::Alchemy, "curvas") => "Cuántas curvas de luz se dibujan a la vez.",
        (P::Alchemy, "tamaño") => "Tamaño de las curvas en pantalla.",
        (P::Alchemy, "partículas") => "Puntos de luz que orbitan alrededor de las curvas.",
        (P::Alchemy, "Estrella en cada golpe") => "Dispara rayos desde el centro en cada beat.",
        (P::Alchemy, "velocidad") => "Velocidad del movimiento y del ciclo de efectos.",
        (P::Alchemy, "fuerza del remolino") => "Cuánto giran y se acercan las copias anteriores.",
        (P::Alchemy, "segundos por modo") => "Cada cuánto cambia el efecto de la estela.",
        (P::Alchemy, "estela") => "Cuánto dura la imagen anterior antes de apagarse.",
        (P::Alchemy, "Sumar caleidoscopio al ciclo") => {
            "Incluye el efecto espejo de 6 lados entre los modos."
        }
        // Ambiente
        (P::Ambience, "escala") => "Tamaño de las formas del gas; más es más chico.",
        (P::Ambience, "brillo de las nubes") => "Qué tan luminoso es el gas.",
        (P::Ambience, "densidad") => "Cuánto gas llena la pantalla; menos deja huecos.",
        (P::Ambience, "turbulencia") => "Cuánto se retuerce y enrosca el gas.",
        (P::Ambience, "velocidad") => "Qué tan rápido fluye el gas.",
        (P::Ambience, "cambio con el golpe") => "Cuánto cambia de forma el gas en cada beat.",
        // Trazo
        (P::Scope, "alto de la onda") => "Altura máxima del osciloscopio.",
        (P::Scope, "expansión") => "Las copias viejas se abren (más de 1) o se cierran.",
        (P::Scope, "desenfoque") => "Qué tan borroso queda el rastro.",
        (P::Scope, "Onda en espejo") => "Agrega una segunda onda invertida con otro tono.",
        // Niagara
        (P::Niagara, "velocidad") => "Qué tan rápido viajan las líneas hacia el centro.",
        (P::Niagara, "separación") => "Distancia al centro donde nacen las líneas.",
        (P::Niagara, "contracción") => "Cuánto se juntan por cuadro; menos es más rápido.",
        (P::Niagara, "escalones") => "Tramos de cada línea; más da una forma más fina.",
        (P::Niagara, "grosor") => "Grosor de las líneas que nacen.",
        (P::Niagara, "Emitir con el volumen, no solo en golpes") => {
            "Sin esto, solo nacen líneas en cada beat."
        }
        // Molino
        (P::Windmill, "giro") => "Velocidad del aspa; negativo gira al revés.",
        (P::Windmill, "ancho de la franja") => "Espacio negro entre las aspas; 0 las pega.",
        (P::Windmill, "líneas por borde") => "Cuántas líneas forman cada aspa.",
        (P::Windmill, "separación de líneas dobles") => "Distancia entre las líneas de un aspa.",
        (P::Windmill, "juntar ondas") => "1 = las dos aspas ondulan juntas, sin abrirse.",
        (P::Windmill, "difusión") => "Qué tanto se esparce la nube que dejan las aspas.",
        (P::Windmill, "amplitud de la onda") => "Qué tanto ondulan las aspas con el audio.",
        (P::Windmill, "estela") => "Cuánto dura la nube antes de apagarse.",
        // X Marca
        (P::XMarks, "giro") => "Velocidad de giro de la X; negativo al revés.",
        (P::XMarks, "brazos") => "Líneas de la figura: 2 es una X, 3 un asterisco.",
        (P::XMarks, "amplitud de la onda") => "Qué tanto ondulan los brazos con el audio.",
        (P::XMarks, "flujo") => "Qué tan rápido se expande y corre el líquido.",
        (P::XMarks, "ondulación") => "Cuánto se agita y enrosca el líquido.",
        (P::XMarks, "pulso") => "Empujón del líquido en cada golpe; 0 lo apaga.",
        (P::XMarks, "estela") => "Cuánto dura el rastro que deja la X.",
        // Partículas
        (P::Particles, "cantidad") => "Número de partículas en pantalla.",
        (P::Particles, "explosión en el golpe") => "Fuerza con la que estallan en cada beat.",
        (P::Particles, "grosor") => "Grosor de cada partícula.",
        (P::Particles, "vida") => "Segundos que dura cada partícula antes de renacer.",
        (P::Particles, "velocidad") => "Qué tan rápido siguen la corriente.",
        (P::Particles, "tamaño de remolinos") => "Tamaño de las corrientes; más es más chico.",
        (P::Particles, "estela") => "Qué tan largo es el rastro de cada partícula.",
        _ => return None,
    })
}

fn common(label: &str) -> Option<&'static str> {
    Some(match label {
        "sensibilidad" => "Qué tanto se mueve la figura con el volumen.",
        "grosor" => "Grosor de las líneas.",
        "suavizado" => "Más suavizado: movimiento más lento y fluido.",
        "estela" => "Cuánto dura el rastro antes de apagarse.",
        "giro" => "Velocidad de giro; negativo gira al revés.",
        "velocidad" => "Qué tan rápido se mueve la escena.",
        // Luz
        "brillo" => "Fuerza del resplandor alrededor de la luz.",
        "umbral" => "Desde qué intensidad brilla; bajo hace brillar todo.",
        "radio del resplandor" => "Qué tan lejos se extiende el resplandor.",
        "exposición" => "Brillo general de la imagen.",
        "contraste" => "Diferencia entre zonas claras y oscuras.",
        "viñeta" => "Oscurece los bordes de la pantalla.",
        // Color
        "ciclo de color" => "Qué tan rápido recorre la paleta.",
        "tono" => "Gira todos los colores en la rueda de color.",
        "saturación" => "Intensidad del color; 0 es blanco y negro.",
        "fuerza del tinte" => "Cuánto tiñe la imagen el color de tinte.",
        "tamaño" => "Agranda o achica toda la escena.",
        "posición horizontal" => "Mueve la escena a la izquierda o a la derecha.",
        "posición vertical" => "Mueve la escena arriba o abajo.",
        "Reflejar izquierda a derecha" => "La mitad izquierda se copia en espejo a la derecha.",
        "Reflejar arriba a abajo" => "La mitad de arriba se copia en espejo abajo.",
        "velocidad de la escena" => "0 congela la animación; 2 la acelera al doble.",
        // Mosaico
        "Repetir en mosaico" => "Repite la escena en una rejilla de copias tenues.",
        "filas" => "Cuántas filas de copias tiene el mosaico.",
        "solape" => "Cuánto se encima cada copia con la de al lado.",
        "opacidad" => "Qué tan visibles son las copias del mosaico.",
        // Reacción
        "ganancia" => "Amplifica el audio antes de dibujarlo.",
        "suavizado del espectro" => "Más suavizado: las frecuencias cambian más lento.",
        "peso de frecuencias" => "Cuánto influyen graves y agudos en forma y color.",
        "sensibilidad a golpes" => "Qué tan fácil detecta un beat; más es más golpes.",
        // Aleatorio
        "Ir cambiando de escena solo" => "Cambia de escena sola cada cierto tiempo.",
        "segundos por escena" => "Tiempo en cada escena antes de cambiar.",
        "Cambiar en el golpe" => "Espera al siguiente beat para cambiar (hasta 4 s).",
        "fundido (s)" => "Duración del fundido a negro entre escenas.",
        "Todas, también las que crees después" => "Incluye toda escena nueva automáticamente.",
        // Ventana
        "Seguir a Spotify" => "Pega la ventana junto a la de Spotify.",
        "Siempre visible" => "La ventana queda encima de las demás.",
        "Ocultar controles en pantalla completa" => "El dock se esconde si no mueves el mouse.",
        "Buscar al abrir" => "Busca una versión nueva cada vez que abres Veeet.",
        "opacidad flotante" => "Transparencia de la ventana en modo flotante.",
        "Ignorar clics en modo flotante" => "Los clics pasan a la ventana de abajo.",
        _ => return None,
    })
}
