import { Button } from './components';

function main() {
    const btn = new Button();
    console.log(btn.render({ label: 'Click me' }));
}
