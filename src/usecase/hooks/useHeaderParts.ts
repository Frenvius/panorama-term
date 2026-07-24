import React from 'react';

import { getHeaderParts, HEADER_PARTS_EVENT, type HeaderParts } from '~/usecase/util/headerParts';

export const useHeaderParts = (): HeaderParts => {
  const [parts, setParts] = React.useState(getHeaderParts);

  React.useEffect(() => {
    const sync = () => setParts(getHeaderParts());
    window.addEventListener(HEADER_PARTS_EVENT, sync);
    return () => window.removeEventListener(HEADER_PARTS_EVENT, sync);
  }, []);

  return parts;
};
