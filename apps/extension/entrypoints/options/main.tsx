import 'omni-ui/styles.css';
import '../../lib/pages.css';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { themePage } from '../../lib/page';
import { Options } from './Options';

themePage();
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Options />
  </StrictMode>,
);
