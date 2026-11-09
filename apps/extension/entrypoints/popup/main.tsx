import 'omni-ui/styles.css';
import '../../lib/pages.css';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { themePage } from '../../lib/page';
import { Popup } from './Popup';

themePage();
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Popup />
  </StrictMode>,
);
