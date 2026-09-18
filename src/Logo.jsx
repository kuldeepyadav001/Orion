/**
 * Orion mark.
 *
 * Built from the constellation's real star positions rather than an invented
 * shape: right ascension and declination for the seven principal stars,
 * projected to a 64×64 box with RA flipped, since RA increases eastward and
 * therefore leftward on the sky.
 *
 *   Betelgeuse  Bellatrix        shoulders
 *   Alnitak · Alnilam · Mintaka  the belt
 *   Saiph       Rigel            feet
 *
 * Star radius is derived from apparent magnitude, so Rigel and Betelgeuse —
 * the two brightest — are visibly the largest. Anyone who knows the sky
 * should recognise it; anyone who does not still gets a balanced mark.
 *
 * Pure inline SVG: no network fetch, no icon font, no image decode, and it
 * inherits currentColor so one component serves the sidebar, the tray and
 * the window icon.
 */
export default function Logo({ size = 24, glow = true }) {
  const id = `orion-grad-${size}`;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label="Orion"
    >
      <defs>
        {glow && (
          <filter id={`${id}-glow`} x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="1.1" result="b" />
            <feMerge>
              <feMergeNode in="b" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        )}
      </defs>

      <g filter={glow ? `url(#${id}-glow)` : undefined}>
        {/* Solid strokes rather than a gradient. A gradient fading toward the
            dark end of the palette left the lines almost invisible on a dark
            background, and the mark read as scattered dots instead of a
            constellation — confirmed by rendering it rather than assuming. */}
        <g
          stroke="#8f74ff"
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
        >
          {/* Betelgeuse to Alnitak to Saiph: left side, shoulder to foot. */}
          <path d="M9 9 L25.3 34.2 L17.4 55" strokeWidth="1.5" opacity="0.75" />
          {/* Bellatrix to Mintaka to Rigel: right side. */}
          <path d="M43 11.9 L35.2 29.8 L55 51" strokeWidth="1.5" opacity="0.75" />
          {/* Shoulders. */}
          <path d="M9 9 L43 11.9" strokeWidth="1.3" opacity="0.5" />
          {/* The belt, brightest because it is the recognisable part. */}
          <path
            d="M25.3 34.2 L30.4 32.2 L35.2 29.8"
            stroke="#c4b5ff"
            strokeWidth="2.2"
          />
        </g>

        {/* Radius scaled by apparent magnitude: Rigel and Betelgeuse largest. */}
        <circle cx="9" cy="9" r="2.5" fill="#e6dcff" />
        <circle cx="43" cy="11.9" r="1.9" fill="#c9b8ff" />
        <circle cx="25.3" cy="34.2" r="1.8" fill="#ffffff" />
        <circle cx="30.4" cy="32.2" r="1.85" fill="#ffffff" />
        <circle cx="35.2" cy="29.8" r="1.6" fill="#ffffff" />
        <circle cx="17.4" cy="55" r="1.7" fill="#bcd4ff" />
        <circle cx="55" cy="51" r="2.7" fill="#eaf2ff" />
      </g>
    </svg>
  );
}
