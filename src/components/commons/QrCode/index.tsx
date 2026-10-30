import React from 'react';
import { encode } from 'uqr';

import styles from './styles.module.scss';

interface QrCodeProps {
  value: string;
  size: number;
}

const modulesPath = (data: boolean[][]): string =>
  data
    .flatMap((row, y) => row.map((dark, x) => (dark ? `M${x},${y}h1v1h-1z` : '')))
    .join('');

const QrCode = ({ value, size }: QrCodeProps) => {
  const qr = React.useMemo(() => encode(value, { ecc: 'M', border: 2 }), [value]);
  const path = React.useMemo(() => modulesPath(qr.data), [qr]);

  return (
    <svg
      width={size}
      height={size}
      role="img"
      aria-label="Pairing QR code"
      className={styles.qr}
      shapeRendering="crispEdges"
      viewBox={`0 0 ${qr.size} ${qr.size}`}
    >
      <path d={path} />
    </svg>
  );
};

export default QrCode;
