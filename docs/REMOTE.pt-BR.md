# Desktop remoto

*Read in [English](REMOTE.md).*

O desktop remoto permite que outro computador mostre e controle um workspace do
Panorama pela rede local, pelo Tailscale ou pelo relay público. O host executa os terminais e mantém o
workspace, os arquivos e os repositórios Git. Mantenha o Panorama aberto no host.

## Parear um desktop

1. No host, abra **Settings → Mobile**, ative **Allow device connections** e escolha
   **Pair a desktop**. Copie a URI de pareamento de desktop inteira.
2. No outro computador, abra **Settings → Remote host**, cole a URI em **Pairing URI**
   e escolha **Pair and connect**.

A URI de pareamento concede acesso total ao host; compartilhe-a com segurança. Para
revogar um desktop, remova-o em **Settings → Mobile → Paired devices** no host.

## Como funciona

O workspace, os terminais, os arquivos e as operações Git pertencem ao host. Cada tela
mantém sua própria posição da câmera do canvas; configurações de aparência e plugins
também são locais. Em alterações simultâneas do workspace, vale o último snapshot salvo.

As conexões usam criptografia X25519 e XSalsa20-Poly1305. A chave pública do host é
fixada no pareamento e verificada na conexão. Um desktop pareado tem controle total:
pode executar comandos e ler e alterar arquivos e repositórios Git como o usuário do
host. O Panorama reconecta automaticamente se a conexão cair.

## Limitações

- Quando os endereços locais não respondem, o desktop pareado conecta pelo relay público. O pareamento ainda exige a rede local.
- O cliente usado mais recentemente para entrada ou foco controla o tamanho do PTY. Os outros clientes se adaptam a esse tamanho.
