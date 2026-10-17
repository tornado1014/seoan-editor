import { mountEditor } from './editor';
import './styles.css';

const root = document.getElementById('app');
if (!root) throw new Error('missing element: app');
mountEditor(root);
