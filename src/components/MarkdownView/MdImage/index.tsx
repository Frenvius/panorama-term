import React from 'react';

import { readFileBytes } from '~/adapter/fs/fs.client';
import { isRemoteUrl, resolveAsset } from '~/usecase/util/markdownRender';

interface MdImageProps {
  src: string;
  alt: string;
  file: string;
}

const MdImage = ({ src, alt, file }: MdImageProps) => {
  const remote = isRemoteUrl(src);
  const [url, setUrl] = React.useState<string | null>(remote ? src : null);

  React.useEffect(() => {
    if (remote || !src) return;
    let alive = true;
    let blob: string | null = null;
    readFileBytes(resolveAsset(file, src))
      .then((buf) => {
        if (!alive) return;
        blob = URL.createObjectURL(new Blob([buf], { type: src.toLowerCase().endsWith('.svg') ? 'image/svg+xml' : '' }));
        setUrl(blob);
      })
      .catch(() => {});
    return () => {
      alive = false;
      if (blob) URL.revokeObjectURL(blob);
    };
  }, [src, file, remote]);

  if (!url) return <span title={src}>{alt}</span>;
  return <img src={url} alt={alt} loading="lazy" />;
};

export default MdImage;
