import React from 'react';
import { LoaderCircle } from 'lucide-react';

import { humanSize } from '~/usecase/util/bytes';
import { gitBlob } from '~/adapter/git/git.client';

import styles from './styles.module.scss';

interface ImagesProps {
  root: string;
  file: string;
  commit?: string;
}

interface Side {
  url: string | null;
  size: number;
}

const EMPTY: Side = { url: null, size: 0 };

const Images = ({ root, file, commit }: ImagesProps) => {
  const [sides, setSides] = React.useState<[Side, Side] | null>(null);
  const [dims, setDims] = React.useState<Record<string, string>>({});

  React.useEffect(() => {
    let alive = true;
    let urls: string[] = [];
    setSides(null);
    setDims({});

    Promise.all([gitBlob(root, file, true, commit), gitBlob(root, file, false, commit)])
      .then(([oldBuf, newBuf]) => {
        if (!alive) return;
        const make = (buf: ArrayBuffer): Side => {
          if (buf.byteLength === 0) return EMPTY;
          const url = URL.createObjectURL(new Blob([buf]));
          urls.push(url);
          return { url, size: buf.byteLength };
        };
        setSides([make(oldBuf), make(newBuf)]);
      })
      .catch(() => alive && setSides([EMPTY, EMPTY]));

    return () => {
      alive = false;
      for (const url of urls) URL.revokeObjectURL(url);
      urls = [];
    };
  }, [root, file, commit]);

  if (!sides) {
    return (
      <div className={styles.notice}>
        <LoaderCircle size={16} strokeWidth={2} className={styles.spinning} />
      </div>
    );
  }

  const pane = (label: string, side: Side) => {
    const onLoad = (e: React.SyntheticEvent<HTMLImageElement>) => {
      const img = e.currentTarget;
      setDims((prev) => ({ ...prev, [label]: `${img.naturalWidth} x ${img.naturalHeight}` }));
    };

    return (
      <div className={styles.pane}>
        <div className={styles.head}>
          <span className={styles.label}>{label}</span>
          {side.url && (
            <span className={styles.meta}>
              {dims[label] ? `${dims[label]} - ` : ''}
              {humanSize(side.size)}
            </span>
          )}
        </div>
        <div className={styles.canvas}>
          {side.url ? <img src={side.url} alt={label} onLoad={onLoad} /> : <span className={styles.empty}>None</span>}
        </div>
      </div>
    );
  };

  return (
    <div className={styles.root}>
      {pane('Before', sides[0])}
      {pane('After', sides[1])}
    </div>
  );
};

export default Images;
