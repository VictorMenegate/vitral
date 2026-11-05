#!/usr/bin/env python3
"""Pasta de demonstração do Vitral: cria do zero uma pasta-mãe para tirar prints na bancada.

Uso: python3 -I ferramentas/demo_vitral.py <pasta_destino> [--16]

Com --16, a pasta ganha mais 12 projetos pequenos (16 ao todo), para o "+N" do trilho.

Cria quatro projetos (loja, site e extensao com git; rascunhos sem git) e a pasta .vitral com
conversas, tarefas, cópias isoladas (git worktree) com mudanças de verdade, pontos de volta,
notas e configuração, no formato de src/modelo.rs (Estado, Chat, Item, Tarefa, Config).

As datas são relativas a agora: gere de novo antes de cada rodada da bancada (as rotinas ativas
saem com "ultima" = hoje, para não rodarem sozinhas; num outro dia elas rodariam).

Se o destino já existe, ele só é apagado e refeito quando é uma vitrine gerada por este script
(tem o arquivo .vitrine-do-vitral); uma pasta qualquer com conteúdo nunca é apagada.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

MARCA = ".vitrine-do-vitral"
AGORA = int(time.time())

# ---------------------------------------------------------------- datas


def meia_noite(instante):
    tm = time.localtime(instante)
    return int(time.mktime((tm.tm_year, tm.tm_mon, tm.tm_mday, 0, 0, 0, 0, 0, -1)))


HOJE = meia_noite(AGORA)


def hoje(minutos):
    """Há `minutos` minutos, mas sem cair em ontem (de madrugada as horas de hoje se espremem)."""
    desde_meia_noite = AGORA - HOJE
    segundos = minutos * 60
    limite = 8 * 3600
    if desde_meia_noite < limite:
        segundos = int(segundos * (desde_meia_noite - 120) / limite)
    return AGORA - max(segundos, 0)


def dia(dias_atras, hora, minuto=0):
    """`dias_atras` dias antes de hoje, nessa hora local."""
    base = time.localtime(HOJE - dias_atras * 86400 + 12 * 3600)
    return int(time.mktime((base.tm_year, base.tm_mon, base.tm_mday, hora, minuto, 0, 0, 0, -1)))


def data_hoje():
    tm = time.localtime(AGORA)
    return f"{tm.tm_year:04}-{tm.tm_mon:02}-{tm.tm_mday:02}"


def fuso():
    deslocamento = -time.altzone if time.localtime(AGORA).tm_isdst > 0 else -time.timezone
    sinal = "+" if deslocamento >= 0 else "-"
    deslocamento = abs(deslocamento)
    return f"{sinal}{deslocamento // 3600:02}{deslocamento % 3600 // 60:02}"


# ---------------------------------------------------------------- git

AMBIENTE_GIT = dict(os.environ)
AMBIENTE_GIT.update(
    {
        # Fora do ~/.gitconfig do usuário: sem assinatura, hooks ou aliases dele.
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_SYSTEM": os.devnull,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "Victor",
        "GIT_AUTHOR_EMAIL": "victor@exemplo.com.br",
        "GIT_COMMITTER_NAME": "Victor",
        "GIT_COMMITTER_EMAIL": "victor@exemplo.com.br",
        "GIT_TERMINAL_PROMPT": "0",
    }
)
SEM_LIXO = [
    ".",
    ":(glob,exclude)**/__pycache__/**",
    ":(glob,exclude)**/*.pyc",
    ":(glob,exclude)**/node_modules/**",
    ":(glob,exclude)**/.pytest_cache/**",
    ":(glob,exclude)**/.DS_Store",
]


def git(pasta, *args, quando=None, indice=None):
    ambiente = dict(AMBIENTE_GIT)
    if quando is not None:
        ambiente["GIT_AUTHOR_DATE"] = ambiente["GIT_COMMITTER_DATE"] = f"{quando} {fuso()}"
    if indice is not None:
        ambiente["GIT_INDEX_FILE"] = indice
    saida = subprocess.run(
        ["git", "-C", pasta, "-c", "core.quotePath=false", *args],
        env=ambiente,
        capture_output=True,
        text=True,
    )
    if saida.returncode != 0:
        sys.exit(f"git {' '.join(args)} em {pasta} falhou:\n{saida.stderr}")
    return saida.stdout.strip()


def escrever(pasta, arquivos):
    for caminho, texto in arquivos.items():
        destino = os.path.join(pasta, caminho)
        os.makedirs(os.path.dirname(destino), exist_ok=True)
        with open(destino, "w", encoding="utf-8") as f:
            f.write(texto)


def commit(pasta, mensagem, arquivos, quando, apagar=()):
    escrever(pasta, arquivos)
    for caminho in apagar:
        os.remove(os.path.join(pasta, caminho))
    git(pasta, "add", "-A")
    git(pasta, "commit", "-q", "--no-verify", "-m", mensagem, quando=quando)
    return git(pasta, "rev-parse", "HEAD")


def foto(pasta, grupo):
    """Ponto de volta como o git::foto do Vitral: commit solto da pasta e a referência no grupo."""
    temporario = tempfile.mkdtemp(prefix="vitrine-indice-")
    indice = os.path.join(temporario, "index")
    try:
        atual = git(pasta, "rev-parse", "--path-format=absolute", "--git-path", "index")
        if os.path.exists(atual):
            shutil.copy(atual, indice)
        git(pasta, "add", "-A", "--", *SEM_LIXO, indice=indice)
        arvore = git(pasta, "write-tree", indice=indice)
        sha = git(pasta, "commit-tree", arvore, "-m", "ponto de volta do Vitral")
    finally:
        shutil.rmtree(temporario, ignore_errors=True)
    git(pasta, "update-ref", f"refs/vitral/pontos/{grupo}/{sha}", sha)
    return sha


def diff_stat(pasta, base):
    """O --stat que o Vitral manda ao coordenador (com os arquivos novos, como o `add -N` dele)."""
    git(pasta, "add", "-N", "--", *SEM_LIXO)
    return git(pasta, "diff", "--stat", base, "--", ".")


def slug(texto):
    """O mesmo slug do motor (nome do branch da tarefa)."""
    trocas = {"á": "a", "à": "a", "â": "a", "ã": "a", "é": "e", "ê": "e", "í": "i", "ó": "o",
              "ô": "o", "õ": "o", "ú": "u", "ü": "u", "ç": "c"}
    s = ""
    for c in texto.lower():
        c = trocas.get(c, c)
        if c.isascii() and c.isalnum():
            s += c
        elif not s.endswith("-"):
            s += "-"
    return s.strip("-")[:30]


def linha_de(texto, trecho):
    """Número (1 em diante) da primeira linha que contém `trecho`."""
    for i, linha in enumerate(texto.splitlines(), start=1):
        if trecho in linha:
            return i
    sys.exit(f"trecho não encontrado: {trecho!r}")


# ---------------------------------------------------------------- itens (enum Item, externally tagged)

_ferramentas = [0]


def usuario(texto, ponto=None, anexos=()):
    return {"Usuario": {"texto": texto, "anexos": list(anexos), "ponto": ponto}}


def entrega(de, texto, ponto=None):
    return {"Entrega": {"de": de, "texto": texto, "ponto": ponto}}


def assistente(texto):
    return {"Assistente": {"texto": texto}}


def aviso(texto, erro=False):
    return {"Aviso": {"texto": texto, "erro": erro}}


def ferramenta(nome, detalhe, resultado, erro=False):
    """`resumo` como claude::resumo_ferramenta: "Nome · detalhe" (sem o prefixo mcp__vitral__)."""
    _ferramentas[0] += 1
    curto = nome.removeprefix("mcp__vitral__")
    return {
        "Ferramenta": {
            "id": f"toolu_01Vit{_ferramentas[0]:04d}{uuid.uuid4().hex[:12]}",
            "nome": nome,
            "resumo": f"{curto} · {detalhe}" if detalhe else curto,
            "resultado": resultado,
            "erro": erro,
        }
    }


def vitral(nome, detalhe, resultado, erro=False):
    return ferramenta("mcp__vitral__" + nome, detalhe, resultado, erro)


# ---------------------------------------------------------------- chats e tarefas


def novo_id():
    return uuid.uuid4().hex[:10]


def horas_dos_itens(itens, criado, atualizado):
    """O `em` de cada item (o `Item.em` do Vitral), espalhado entre a criação e o último uso do
    chat: a conversa mostra a hora de cada mensagem e o separador de data onde o dia muda."""
    for i, item in enumerate(itens):
        (campos,) = item.values()
        passo = 0 if len(itens) < 2 else round((atualizado - criado) * i / (len(itens) - 1))
        campos["em"] = criado + passo


def chat(id, projeto, titulo, papel, cwd, criado, atualizado, itens, **extra):
    horas_dos_itens(itens, criado, atualizado)
    c = {
        "id": id,
        "projeto": projeto,
        "titulo": titulo,
        "papel": papel,
        "pai": None,
        "tarefa": None,
        "sessao": str(uuid.uuid4()),
        "sessao_criada": True,
        "cwd": cwd,
        "modelo": None,
        "modelo_atual": "claude-opus-5-5" if papel == "Coordenador" else "claude-sonnet-5",
        "permissao": None,
        "permitidas": [],
        "esforco": None,
        "conta": None,
        "contexto": None,
        "janela": 200000,
        "comandos": ["clear", "compact", "context", "cost", "init", "review", "pr-comments", "security-review"],
        "criado_em": criado,
        "atualizado_em": atualizado,
        "custo_usd": 0.0,
        "fixado": False,
        "arquivado": False,
        "lido_em": atualizado,
        "turno_aberto": False,
        "modo_antes_do_plano": None,
    }
    c.update(extra)
    return c, itens


def tarefa(id, projeto, coordenador, trabalhador, titulo, objetivo, criterios, verificacao, estado,
           criada, **extra):
    t = {
        "id": id,
        "projeto": projeto,
        "coordenador": coordenador,
        "trabalhador": trabalhador,
        "titulo": titulo,
        "objetivo": objetivo,
        "criterios": criterios,
        "verificacao": verificacao,
        "estado": estado,
        "rodada": 1,
        "worktree": None,
        "branch": None,
        "base": None,
        "copia_limpa": False,
        "progresso": None,
        "relatorio": None,
        "ultima_verificacao": None,
        "criada_em": criada,
        "revisao": None,
        "sobreposicao": [],
        "preparacao": None,
        "parada": None,
        "entrega_pendente": None,
        "preparada": True,
        "ressalva": None,
    }
    t.update(extra)
    return t


# Projetos extras da variante --16: só uma pasta com um arquivo, sem git.
EXTRAS = [
    ("blog", "index.md", "# Blog\n"),
    ("api-precos", "main.py", "print('preços')\n"),
    ("app-celular", "package.json", '{"name": "app-celular"}\n'),
    ("painel-vendas", "index.html", "<h1>Vendas</h1>\n"),
    ("bot-telegram", "bot.py", "# bot\n"),
    ("dotfiles", "README.md", "# dotfiles\n"),
    ("jogo-da-velha", "Cargo.toml", '[package]\nname = "jogo"\n'),
    ("notas-aulas", "aula-01.md", "# Aula 1\n"),
    ("scripts", "backup.sh", "#!/bin/sh\n"),
    ("loja-antiga", "manage.py", "# django\n"),
    ("portfolio", "index.html", "<h1>Portfólio</h1>\n"),
    ("extensao-tv", "metadata.json", '{"uuid": "tv@victor"}\n'),
]


def projetos_extras(raiz):
    for nome, arquivo, conteudo in EXTRAS:
        os.makedirs(os.path.join(raiz, nome), exist_ok=True)
        with open(os.path.join(raiz, nome, arquivo), "w", encoding="utf-8") as f:
            f.write(conteudo)


def pedido_da_tarefa(t):
    """O primeiro pedido do trabalhador, como prompts::tarefa."""
    criterios = "".join(f"- {c}\n" for c in t["criterios"])
    verificacao = (
        f"\nQuando você terminar, o Vitral vai rodar `{t['verificacao']}` na sua pasta.\n"
        if t["verificacao"]
        else ""
    )
    return (
        f"Tarefa #{t['id']}: {t['titulo']}\n\nObjetivo:\n{t['objetivo']}\n\n"
        f"Critérios de aceitação:\n{criterios}{verificacao}"
    )


def verificacao(comando, passou, saida, duracao):
    return {"comando": comando, "passou": passou, "saida": saida, "duracao_s": duracao}


def tempo_curto(s):
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{s // 60}min{s % 60:02}s"
    return f"{s // 3600}h{s % 3600 // 60:02}"


def texto_revisao(r):
    """crate::revisor::texto."""
    texto = "Revisor independente: {} — {}\n".format(
        "APROVARIA" if r["veredito"] == "aprovar" else "PEDE AJUSTES", r["resumo"].strip()
    )
    for n, p in enumerate(r["problemas"], start=1):
        onde = f"{p['arquivo']}:{p['linha']}" if p.get("linha") else p["arquivo"]
        texto += f"{n}. [{p['gravidade']}] {onde}: {p['descricao']}\n"
    return texto


def entrega_do_vitral(t, relatorio, stat, verif=None, revisao=None, rodada=1):
    """A mensagem que tarefa_terminou entrega ao coordenador."""
    texto = (
        f"A tarefa #{t['id']} \"{t['titulo']}\" terminou a rodada {rodada}.\n\n"
        f"Relatório do trabalhador:\n{relatorio}\n"
    )
    texto += f"\nArquivos alterados (git diff --stat):\n{stat}\n"
    if verif:
        texto += "\nVerificação `{}`: {} em {}\n```\n{}\n```\n".format(
            verif["comando"], "PASSOU" if verif["passou"] else "FALHOU",
            tempo_curto(verif["duracao_s"]), verif["saida"].strip()
        )
    if revisao:
        texto += "\n" + texto_revisao(revisao)
    texto += (
        f"\nRevise (ver_diff {t['id']}; se o diff for grande vem um resumo e você pede os arquivos "
        "que importam) e decida: aprovar_tarefa ou pedir_ajuste."
    )
    return entrega("Vitral", texto)


def criou(t, trabalhador, extra=""):
    return vitral(
        "criar_tarefa", t["titulo"],
        f"Tarefa #{t['id']} criada; trabalhador {trabalhador} iniciado.{extra} O relatório chega "
        "para você automaticamente quando ele terminar.",
    )


# ================================================================= projetos

LOJA_INICIAL = {
    ".gitignore": ".venv/\n__pycache__/\n*.pyc\n.env\ndb.sqlite3\nmedia/\n",
    "README.md": """# Loja

Loja virtual em Django: catálogo de produtos, carrinho na sessão e pedidos com pagamento na
entrega.

## Rodar

```sh
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
.venv/bin/python manage.py migrate
.venv/bin/python manage.py runserver
```

## Testes

```sh
.venv/bin/python manage.py test produtos
```

## Estrutura

| Pasta | O que tem |
| --- | --- |
| `loja/` | configurações e rotas do projeto |
| `produtos/` | modelos, views, templates e testes da loja |
""",
    "requirements.txt": "Django==5.2.7\nPillow==11.3.0\ngunicorn==23.0.0\nwhitenoise==6.9.0\n",
    "manage.py": """#!/usr/bin/env python
import os
import sys


def main():
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "loja.settings")
    from django.core.management import execute_from_command_line

    execute_from_command_line(sys.argv)


if __name__ == "__main__":
    main()
""",
    "loja/__init__.py": "",
    "loja/settings.py": """from pathlib import Path
import os

BASE_DIR = Path(__file__).resolve().parent.parent
SECRET_KEY = os.environ.get("DJANGO_SECRET_KEY", "dev-inseguro")
DEBUG = os.environ.get("DJANGO_DEBUG", "1") == "1"
ALLOWED_HOSTS = os.environ.get("DJANGO_HOSTS", "localhost,127.0.0.1").split(",")

INSTALLED_APPS = [
    "django.contrib.admin",
    "django.contrib.auth",
    "django.contrib.contenttypes",
    "django.contrib.sessions",
    "django.contrib.messages",
    "django.contrib.staticfiles",
    "produtos",
]

MIDDLEWARE = [
    "django.middleware.security.SecurityMiddleware",
    "whitenoise.middleware.WhiteNoiseMiddleware",
    "django.contrib.sessions.middleware.SessionMiddleware",
    "django.middleware.common.CommonMiddleware",
    "django.middleware.csrf.CsrfViewMiddleware",
    "django.contrib.auth.middleware.AuthenticationMiddleware",
    "django.contrib.messages.middleware.MessageMiddleware",
]

ROOT_URLCONF = "loja.urls"
TEMPLATES = [
    {
        "BACKEND": "django.template.backends.django.DjangoTemplates",
        "APP_DIRS": True,
        "OPTIONS": {"context_processors": [
            "django.template.context_processors.request",
            "django.contrib.auth.context_processors.auth",
            "django.contrib.messages.context_processors.messages",
        ]},
    }
]
DATABASES = {"default": {"ENGINE": "django.db.backends.sqlite3", "NAME": BASE_DIR / "db.sqlite3"}}
LANGUAGE_CODE = "pt-br"
TIME_ZONE = "America/Sao_Paulo"
USE_TZ = True
STATIC_URL = "static/"
STATIC_ROOT = BASE_DIR / "static"
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"
""",
    "loja/urls.py": """from django.contrib import admin
from django.urls import include, path

urlpatterns = [
    path("admin/", admin.site.urls),
    path("", include("produtos.urls")),
]
""",
    "produtos/__init__.py": "",
    "produtos/models.py": """from decimal import Decimal

from django.db import models


class Produto(models.Model):
    nome = models.CharField(max_length=120)
    descricao = models.TextField(blank=True)
    preco = models.DecimalField(max_digits=10, decimal_places=2)
    estoque = models.PositiveIntegerField(default=0)
    ativo = models.BooleanField(default=True)

    def __str__(self):
        return self.nome


class Pedido(models.Model):
    ABERTO, PAGO, ENVIADO = "aberto", "pago", "enviado"
    STATUS = [(ABERTO, "Aberto"), (PAGO, "Pago"), (ENVIADO, "Enviado")]

    cliente = models.ForeignKey("auth.User", on_delete=models.PROTECT, related_name="pedidos")
    status = models.CharField(max_length=10, choices=STATUS, default=ABERTO)
    criado_em = models.DateTimeField(auto_now_add=True)

    def total(self) -> Decimal:
        return sum((i.subtotal() for i in self.itens.all()), Decimal("0"))


class ItemPedido(models.Model):
    pedido = models.ForeignKey(Pedido, on_delete=models.CASCADE, related_name="itens")
    produto = models.ForeignKey(Produto, on_delete=models.PROTECT)
    quantidade = models.PositiveIntegerField(default=1)
    preco = models.DecimalField(max_digits=10, decimal_places=2)

    def subtotal(self) -> Decimal:
        return self.preco * self.quantidade
""",
    "produtos/admin.py": """from django.contrib import admin

from .models import ItemPedido, Pedido, Produto


@admin.register(Produto)
class ProdutoAdmin(admin.ModelAdmin):
    list_display = ("nome", "preco", "estoque", "ativo")
    search_fields = ("nome",)


class ItemInline(admin.TabularInline):
    model = ItemPedido
    extra = 0


@admin.register(Pedido)
class PedidoAdmin(admin.ModelAdmin):
    list_display = ("id", "cliente", "status", "criado_em")
    list_filter = ("status",)
    inlines = [ItemInline]
""",
    "produtos/urls.py": """from django.urls import path

from . import views

urlpatterns = [
    path("", views.lista, name="lista"),
    path("carrinho/", views.carrinho, name="carrinho"),
    path("carrinho/adicionar/<int:produto_id>/", views.adicionar, name="adicionar"),
]
""",
    "produtos/views.py": """from decimal import Decimal

from django.shortcuts import get_object_or_404, redirect, render

from .models import Produto


def lista(request):
    produtos = Produto.objects.filter(ativo=True).order_by("nome")
    return render(request, "produtos/lista.html", {"produtos": produtos})


def adicionar(request, produto_id):
    produto = get_object_or_404(Produto, pk=produto_id, ativo=True)
    carrinho = request.session.setdefault("carrinho", {})
    carrinho[str(produto.id)] = carrinho.get(str(produto.id), 0) + 1
    request.session.modified = True
    return redirect("carrinho")
""",
    "produtos/templates/produtos/lista.html": """{% extends "base.html" %}
{% block conteudo %}
<h1>Produtos</h1>
<ul class="grade">
  {% for p in produtos %}
  <li class="cartao">
    <h2>{{ p.nome }}</h2>
    <p class="preco">R$ {{ p.preco }}</p>
    <a class="botao" href="{% url 'adicionar' p.id %}">Adicionar ao carrinho</a>
  </li>
  {% empty %}
  <li>Nenhum produto por enquanto.</li>
  {% endfor %}
</ul>
{% endblock %}
""",
    "produtos/templates/base.html": """<!doctype html>
<html lang="pt-br">
<head>
  <meta charset="utf-8">
  <title>{% block titulo %}Loja{% endblock %}</title>
</head>
<body>
  <header><a href="/">Loja</a> · <a href="{% url 'carrinho' %}">Carrinho</a></header>
  <main>{% block conteudo %}{% endblock %}</main>
</body>
</html>
""",
    "produtos/tests.py": """from decimal import Decimal

from django.test import TestCase

from .models import Produto


class ProdutoTests(TestCase):
    def test_lista_mostra_so_ativos(self):
        Produto.objects.create(nome="Caneca", preco=Decimal("39.90"))
        Produto.objects.create(nome="Velho", preco=Decimal("1.00"), ativo=False)
        resposta = self.client.get("/")
        self.assertContains(resposta, "Caneca")
        self.assertNotContains(resposta, "Velho")
""",
}

LOJA_CARRINHO = {
    "produtos/views.py": LOJA_INICIAL["produtos/views.py"] + """

def carrinho(request):
    itens, subtotal = [], Decimal("0")
    for id, quantidade in request.session.get("carrinho", {}).items():
        produto = get_object_or_404(Produto, pk=id)
        itens.append((produto, quantidade, produto.preco * quantidade))
        subtotal += produto.preco * quantidade
    return render(request, "produtos/carrinho.html", {"itens": itens, "subtotal": subtotal})
""",
    "produtos/templates/produtos/carrinho.html": """{% extends "base.html" %}
{% block conteudo %}
<h1>Carrinho</h1>
<table class="carrinho">
  {% for produto, quantidade, valor in itens %}
  <tr><td>{{ produto.nome }}</td><td>{{ quantidade }}×</td><td>R$ {{ valor }}</td></tr>
  {% empty %}
  <tr><td colspan="3">O carrinho está vazio.</td></tr>
  {% endfor %}
</table>
<p class="total">Total: <strong>R$ {{ subtotal }}</strong></p>
<a class="botao" href="/pedido/fechar/">Fechar pedido</a>
{% endblock %}
""",
}

LOJA_CSV = {
    "produtos/admin.py": LOJA_INICIAL["produtos/admin.py"].replace(
        "from django.contrib import admin\n",
        "import csv\n\nfrom django.contrib import admin\nfrom django.http import HttpResponse\n",
    ).replace(
        "    inlines = [ItemInline]\n",
        """    inlines = [ItemInline]
    actions = ["exportar_csv"]

    @admin.action(description="Exportar selecionados em CSV")
    def exportar_csv(self, request, queryset):
        resposta = HttpResponse(content_type="text/csv; charset=utf-8")
        resposta["Content-Disposition"] = 'attachment; filename="pedidos.csv"'
        escritor = csv.writer(resposta, delimiter=";")
        escritor.writerow(["pedido", "cliente", "status", "criado em", "total"])
        for p in queryset:
            escritor.writerow([p.id, p.cliente, p.get_status_display(), p.criado_em.strftime("%d/%m/%Y"), p.total()])
        return resposta
""",
    ),
}


def loja_indices(admin):
    return {
        "produtos/admin.py": admin.replace(
            '    list_display = ("id", "cliente", "status", "criado_em")\n',
            '    list_display = ("id", "cliente", "status", "criado_em")\n'
            '    list_select_related = ("cliente",)\n',
        ),
        "produtos/models.py": LOJA_INICIAL["produtos/models.py"].replace(
            "    criado_em = models.DateTimeField(auto_now_add=True)\n",
            "    criado_em = models.DateTimeField(auto_now_add=True, db_index=True)\n\n"
            "    class Meta:\n        indexes = [models.Index(fields=[\"status\", \"criado_em\"])]\n",
        ),
    }


CUPOM_MODELO = """

class Cupom(models.Model):
    PERCENTUAL, FIXO = "percentual", "fixo"
    TIPOS = [(PERCENTUAL, "Percentual"), (FIXO, "Valor fixo")]

    codigo = models.CharField(max_length=30, unique=True)
    tipo = models.CharField(max_length=10, choices=TIPOS, default=PERCENTUAL)
    valor = models.DecimalField(max_digits=10, decimal_places=2)
    validade = models.DateField(null=True, blank=True)
    limite_usos = models.PositiveIntegerField(null=True, blank=True)
    usos = models.PositiveIntegerField(default=0)

    def save(self, *args, **kwargs):
        self.codigo = self.codigo.strip().upper()
        super().save(*args, **kwargs)

    def __str__(self):
        return self.codigo
"""

LOJA_TESTES_CUPOM = LOJA_INICIAL["produtos/tests.py"].replace(
    "from .models import Produto\n", "from .models import Cupom, Produto\n"
) + """

class CupomTests(TestCase):
    def test_codigo_vira_maiusculo(self):
        cupom = Cupom.objects.create(codigo=" verao10 ", valor=Decimal("10"))
        self.assertEqual(cupom.codigo, "VERAO10")

    def test_codigo_unico(self):
        Cupom.objects.create(codigo="BEMVINDO", valor=Decimal("5"))
        with self.assertRaises(Exception):
            Cupom.objects.create(codigo="bemvindo", valor=Decimal("5"))
"""

# Tarefa #4: a mudança que fica na cópia, para o Revisar.
CUPONS_PY = '''"""Regras dos cupons de desconto do carrinho."""

from datetime import datetime
from decimal import Decimal

from .models import Cupom


class CupomInvalido(Exception):
    """O código não existe, venceu ou já foi usado o máximo de vezes."""


def aplicar_cupom(codigo: str, subtotal: Decimal) -> tuple[Cupom, Decimal]:
    """Valida o cupom e devolve ele com o desconto sobre o subtotal."""
    cupom = Cupom.objects.filter(codigo__iexact=codigo.strip()).first()
    if cupom is None:
        raise CupomInvalido("Cupom não encontrado.")
    if cupom.validade and cupom.validade < datetime.now():
        raise CupomInvalido("Este cupom venceu.")
    if cupom.limite_usos and cupom.usos >= cupom.limite_usos:
        raise CupomInvalido("Este cupom já foi usado o máximo de vezes.")
    return cupom, desconto(cupom, subtotal)


def desconto(cupom: Cupom, subtotal: Decimal) -> Decimal:
    if cupom.tipo == Cupom.PERCENTUAL:
        return subtotal * cupom.valor / 100
    return min(cupom.valor, subtotal)
'''

VIEW_CARRINHO_NOVA = """

def carrinho(request):
    itens, subtotal = [], Decimal("0")
    for id, quantidade in request.session.get("carrinho", {}).items():
        produto = get_object_or_404(Produto, pk=id)
        itens.append((produto, quantidade, produto.preco * quantidade))
        subtotal += produto.preco * quantidade

    erro_cupom, desconto = None, Decimal("0")
    if request.method == "POST" and "cupom" in request.POST:
        request.session["cupom"] = request.POST["cupom"].strip()
    codigo = request.session.get("cupom")
    if codigo:
        try:
            _, desconto = aplicar_cupom(codigo, subtotal)
        except CupomInvalido as erro:
            erro_cupom = str(erro)
    contexto = {
        "itens": itens,
        "subtotal": subtotal,
        "cupom": codigo,
        "desconto": desconto,
        "erro_cupom": erro_cupom,
        "total": subtotal - desconto,
    }
    return render(request, "produtos/carrinho.html", contexto)
"""

CARRINHO_HTML_NOVO = """{% extends "base.html" %}
{% block conteudo %}
<h1>Carrinho</h1>
<table class="carrinho">
  {% for produto, quantidade, valor in itens %}
  <tr><td>{{ produto.nome }}</td><td>{{ quantidade }}×</td><td>R$ {{ valor }}</td></tr>
  {% empty %}
  <tr><td colspan="3">O carrinho está vazio.</td></tr>
  {% endfor %}
</table>
<form method="post" class="cupom">
  {% csrf_token %}
  <label for="cupom">Cupom de desconto</label>
  <input id="cupom" name="cupom" value="{{ cupom|default:'' }}" autocomplete="off">
  <button type="submit">Aplicar</button>
  {% if erro_cupom %}<p class="erro">{{ erro_cupom }}</p>{% endif %}
</form>
<p>Subtotal: R$ {{ subtotal }}</p>
{% if desconto %}<p class="desconto">Desconto ({{ cupom }}): − R$ {{ desconto }}</p>{% endif %}
<p class="total">Total: <strong>R$ {{ total }}</strong></p>
<a class="botao" href="/pedido/fechar/">Fechar pedido</a>
{% endblock %}
"""

TESTES_CARRINHO = """

class CarrinhoTests(TestCase):
    def setUp(self):
        self.produto = Produto.objects.create(nome="Caneca", preco=Decimal("50.00"))
        self.client.get(f"/carrinho/adicionar/{self.produto.id}/")

    def test_cupom_percentual(self):
        Cupom.objects.create(codigo="DEZ", valor=Decimal("10"))
        resposta = self.client.post("/carrinho/", {"cupom": "dez"})
        self.assertEqual(resposta.context["total"], Decimal("45.00"))

    def test_cupom_fixo_nao_passa_do_subtotal(self):
        Cupom.objects.create(codigo="CEM", tipo=Cupom.FIXO, valor=Decimal("100"))
        resposta = self.client.post("/carrinho/", {"cupom": "CEM"})
        self.assertEqual(resposta.context["total"], Decimal("0.00"))

    def test_cupom_inexistente(self):
        resposta = self.client.post("/carrinho/", {"cupom": "NADA"})
        self.assertEqual(resposta.context["erro_cupom"], "Cupom não encontrado.")

    def test_limite_de_usos(self):
        Cupom.objects.create(codigo="UMA", valor=Decimal("5"), limite_usos=1, usos=1)
        resposta = self.client.post("/carrinho/", {"cupom": "UMA"})
        self.assertIn("máximo", resposta.context["erro_cupom"])

    def test_cupom_expirado_nao_vale(self):
        Cupom.objects.create(codigo="VERAO10", valor=Decimal("10"), validade=date(2026, 3, 20))
        resposta = self.client.post("/carrinho/", {"cupom": "VERAO10"})
        self.assertEqual(resposta.context["erro_cupom"], "Este cupom venceu.")
"""

SITE_INICIAL = {
    "README.md": """# Site

Site institucional estático (HTML e CSS puros, sem build). Para ver localmente:

```sh
python3 -m http.server 8080
```
""",
    "index.html": """<!doctype html>
<html lang="pt-br">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Ateliê Vitral · Peças em vidro feitas à mão</title>
  <link rel="stylesheet" href="css/estilo.css">
</head>
<body>
  <header class="topo">
    <a class="marca" href="index.html">Ateliê Vitral</a>
    <nav>
      <a href="index.html" aria-current="page">Início</a>
      <a href="sobre.html">Sobre</a>
    </nav>
  </header>
  <main>
    <section class="destaque">
      <h1>Vidro, luz e cor</h1>
      <p>Luminárias, mosaicos e janelas sob encomenda, feitos peça por peça.</p>
      <a class="botao" href="sobre.html">Conheça o ateliê</a>
    </section>
  </main>
  <footer>© Ateliê Vitral</footer>
  <script src="js/menu.js"></script>
</body>
</html>
""",
    "sobre.html": """<!doctype html>
<html lang="pt-br">
<head>
  <meta charset="utf-8">
  <title>Sobre · Ateliê Vitral</title>
  <link rel="stylesheet" href="css/estilo.css">
</head>
<body>
  <main>
    <h1>Sobre o ateliê</h1>
    <p>Trabalhamos com vidro desde 2009, em Belo Horizonte.</p>
  </main>
</body>
</html>
""",
    "css/estilo.css": """:root {
  --fundo: #0f1422;
  --texto: #e6ebf5;
  --destaque: #5b8cff;
  --raio: 12px;
}

* { box-sizing: border-box; }

body {
  margin: 0;
  font-family: system-ui, sans-serif;
  background: var(--fundo);
  color: var(--texto);
}

.topo {
  display: flex;
  justify-content: space-between;
  padding: 1rem 2rem;
}

.destaque {
  padding: 6rem 2rem;
  text-align: center;
}

.botao {
  display: inline-block;
  padding: .75rem 1.25rem;
  border-radius: var(--raio);
  background: var(--destaque);
  color: white;
  text-decoration: none;
}
""",
    "js/menu.js": """// Menu recolhível no celular.
const nav = document.querySelector(".topo nav");
const botao = document.createElement("button");
botao.textContent = "Menu";
botao.setAttribute("aria-expanded", "false");
botao.addEventListener("click", () => {
  const aberto = nav.classList.toggle("aberto");
  botao.setAttribute("aria-expanded", String(aberto));
});
nav.before(botao);
""",
}

CONTATO_HTML = """<!doctype html>
<html lang="pt-br">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Contato · Ateliê Vitral</title>
  <link rel="stylesheet" href="css/estilo.css">
</head>
<body>
  <main class="contato">
    <h1>Fale com a gente</h1>
    <p>Conte o que você imagina; respondemos em até dois dias úteis.</p>
    <form action="https://formsubmit.co/contato@atelie-vitral.com.br" method="post" novalidate>
      <label for="nome">Nome</label>
      <input id="nome" name="nome" autocomplete="name" required>

      <label for="email">E-mail</label>
      <input id="email" name="email" type="email" autocomplete="email" required>

      <label for="mensagem">Mensagem</label>
      <textarea id="mensagem" name="mensagem" rows="6" required></textarea>

      <p class="dica" id="dica-prazo">Encomendas levam de 3 a 6 semanas.</p>
      <button class="botao" type="submit" aria-describedby="dica-prazo">Enviar</button>
    </form>
  </main>
</body>
</html>
"""

CSS_FORMULARIO = """
.contato {
  max-width: 36rem;
  margin: 4rem auto;
  padding: 0 1.5rem;
}

.contato form {
  display: grid;
  gap: .5rem;
}

.contato input,
.contato textarea {
  padding: .75rem;
  border: 1px solid #2a3350;
  border-radius: var(--raio);
  background: #161d30;
  color: var(--texto);
}

.contato ::placeholder { color: #5c6680; }

.contato .dica {
  font-size: .875rem;
  opacity: .8;
}
"""

VALIDACAO_JS = """// Validação do formulário de contato antes de enviar.
export function erros({ nome, email, mensagem }) {
  const lista = [];
  if (!nome.trim()) lista.push(["nome", "Diga como podemos te chamar."]);
  if (!/^[^@\\s]+@[^@\\s]+$/.test(email)) lista.push(["email", "Esse e-mail não parece certo."]);
  if (mensagem.trim().length < 10) lista.push(["mensagem", "Conte um pouco mais (10 letras ou mais)."]);
  return lista;
}

const form = document.querySelector(".contato form");
form?.addEventListener("submit", (evento) => {
  const dados = Object.fromEntries(new FormData(form));
  const lista = erros(dados);
  for (const campo of form.querySelectorAll("[aria-invalid]")) campo.removeAttribute("aria-invalid");
  if (lista.length) {
    evento.preventDefault();
    for (const [nome] of lista) form.elements[nome].setAttribute("aria-invalid", "true");
    form.elements[lista[0][0]].focus();
  }
});
"""

VALIDACAO_TESTE = """import { test } from "node:test";
import assert from "node:assert/strict";
import { erros } from "./validacao.js";

test("aceita formulário completo", () => {
  assert.deepEqual(erros({ nome: "Ana", email: "ana@exemplo.com", mensagem: "Quero um vitral" }), []);
});

test("recusa e-mail sem domínio", () => {
  assert.equal(erros({ nome: "Ana", email: "ana@", mensagem: "Quero um vitral" }).length, 1);
});
"""

EXTENSAO_INICIAL = {
    "metadata.json": json.dumps(
        {
            "uuid": "desfoque-janelas@victor.local",
            "name": "Desfoque atrás das janelas",
            "description": "Desfoca o fundo atrás de janelas transparentes, como o VS Code.",
            "shell-version": ["48", "49"],
            "settings-schema": "org.gnome.shell.extensions.desfoque",
            "url": "https://github.com/victor/desfoque-janelas",
        },
        indent=2,
        ensure_ascii=False,
    ) + "\n",
    "extension.js": """import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

export default class DesfoqueJanelas extends Extension {
    enable() {
        this._configuracoes = this.getSettings();
        this._efeitos = new Map();
        this._sinal = global.display.connect('window-created', (_, janela) => this._acompanhar(janela));
        for (const ator of global.get_window_actors())
            this._acompanhar(ator.meta_window);
    }

    disable() {
        global.display.disconnect(this._sinal);
        for (const [ator, efeito] of this._efeitos)
            ator.remove_effect(efeito);
        this._efeitos = null;
        this._configuracoes = null;
    }

    _acompanhar(janela) {
        const apps = this._configuracoes.get_strv('apps');
        if (!apps.includes(janela.get_wm_class()))
            return;
        const ator = janela.get_compositor_private();
        const efeito = new Shell.BlurEffect({
            radius: this._configuracoes.get_int('raio'),
            brightness: 0.9,
            mode: Shell.BlurMode.BACKGROUND,
        });
        ator.add_effect(efeito);
        this._efeitos.set(ator, efeito);
    }
}
""",
    "stylesheet.css": "/* A extensão não muda o tema do Shell. */\n",
    "schemas/org.gnome.shell.extensions.desfoque.gschema.xml": """<?xml version="1.0" encoding="UTF-8"?>
<schemalist>
  <schema id="org.gnome.shell.extensions.desfoque" path="/org/gnome/shell/extensions/desfoque/">
    <key name="apps" type="as">
      <default>['Code']</default>
      <summary>Programas com o fundo desfocado</summary>
    </key>
    <key name="raio" type="i">
      <default>30</default>
      <summary>Raio do desfoque</summary>
    </key>
  </schema>
</schemalist>
""",
    "README.md": """# Desfoque atrás das janelas

Extensão do GNOME Shell que desfoca o que fica atrás de janelas transparentes (VS Code com
transparência, terminais).

```sh
gnome-extensions pack --extra-source=schemas
gnome-extensions install --force desfoque-janelas@victor.local.shell-extension.zip
```
""",
}

PREFS_JS = """import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

export default class Preferencias extends ExtensionPreferences {
    fillPreferencesWindow(janela) {
        const configuracoes = this.getSettings();
        const pagina = new Adw.PreferencesPage();
        const grupo = new Adw.PreferencesGroup({title: 'Desfoque'});
        const raio = new Adw.SpinRow({
            title: 'Raio',
            adjustment: new Gtk.Adjustment({lower: 0, upper: 100, step_increment: 5}),
        });
        configuracoes.bind('raio', raio, 'value', 0);
        grupo.add(raio);
        pagina.add(grupo);
        janela.add(pagina);
    }
}
"""

RASCUNHOS = {
    "ideias-videos.md": """# Ideias de vídeos

- Como funciona um vitral (chumbo, solda, pátina)
- Do desenho ao vidro cortado: timelapse de uma luminária
- 5 erros de quem começa a cortar vidro
- Visita a uma fornecedora de vidro catedral
- Restaurando a janela da casa da vó
""",
    "roteiro-ep12.md": """# Episódio 12 · Restaurando a janela da casa da vó

1. Abertura: a janela como chegou (rachada, chumbo oxidado)
2. Desmontar e numerar as peças
3. Copiar o desenho para o papel
4. Cortar as peças que faltam
5. Montar, soldar e passar a pátina
6. A janela de volta no lugar
""",
    "compras.txt": "chumbo H 6 mm (2 rolos)\nsolda 60/40\npátina preta\nvidro catedral âmbar 30x30\nfita de cobre 5 mm\n",
}


# ================================================================= a vitrine


def gerar(raiz):
    ws_vitral = os.path.join(raiz, ".vitral")
    proj = {nome: os.path.join(raiz, nome) for nome in ["loja", "site", "extensao", "rascunhos"]}
    for pasta in proj.values():
        os.makedirs(pasta)

    # ---------- loja: histórico ----------
    loja = proj["loja"]
    git(loja, "init", "-q", "-b", "main")
    commit(loja, "Projeto inicial da loja", LOJA_INICIAL, dia(10, 14, 20))
    commit(loja, "Carrinho na sessão", LOJA_CARRINHO, dia(8, 10, 5))
    commit(loja, "Exportar pedidos em CSV no admin", LOJA_CSV, dia(6, 16, 40))
    admin_csv = LOJA_CSV["produtos/admin.py"]
    indices = loja_indices(admin_csv)
    sha_t2 = commit(loja, "Índices e select_related no admin (tarefa #2)", indices, dia(1, 15, 12))
    modelos_com_cupom = indices["produtos/models.py"] + CUPOM_MODELO
    sha_t3 = commit(
        loja,
        "Modelo Cupom e validação (tarefa #3)",
        {"produtos/models.py": modelos_com_cupom, "produtos/tests.py": LOJA_TESTES_CUPOM},
        hoje(38),
    )
    escrever(loja, {".env": "DJANGO_SECRET_KEY=troque-isto-na-producao\nDJANGO_DEBUG=1\n"})

    # ---------- site ----------
    site = proj["site"]
    git(site, "init", "-q", "-b", "main")
    commit(site, "Site inicial", SITE_INICIAL, dia(12, 9, 30))
    commit(
        site, "Página Sobre com a história do ateliê",
        {"sobre.html": SITE_INICIAL["sobre.html"].replace(
            "    <p>Trabalhamos com vidro desde 2009, em Belo Horizonte.</p>\n",
            "    <p>Trabalhamos com vidro desde 2009, em Belo Horizonte.</p>\n"
            "    <p>Cada peça é desenhada com você e montada à mão, com chumbo ou fita de cobre.</p>\n")},
        dia(5, 11, 0),
    )
    css_fonte = SITE_INICIAL["css/estilo.css"].replace(
        "  font-family: system-ui, sans-serif;\n",
        "  font-family: \"Inter\", system-ui, sans-serif;\n",
    ) + "\n.marca {\n  font-family: \"Fraunces\", Georgia, serif;\n  font-size: 1.5rem;\n}\n"
    commit(site, "Fonte serifada no cabeçalho", {"css/estilo.css": css_fonte}, dia(1, 18, 45))

    # ---------- extensao ----------
    extensao = proj["extensao"]
    git(extensao, "init", "-q", "-b", "main")
    commit(extensao, "Extensão de desfoque inicial", EXTENSAO_INICIAL, dia(15, 20, 10))
    commit(extensao, "Preferências com Adw", {"prefs.js": PREFS_JS}, dia(7, 21, 30))
    meta50 = EXTENSAO_INICIAL["metadata.json"].replace('"48",\n    "49"', '"48",\n    "49",\n    "50"')
    ext50 = EXTENSAO_INICIAL["extension.js"].replace(
        "            radius: this._configuracoes.get_int('raio'),\n",
        "            // GNOME 50: o raio virou \"radius\" em pixels físicos.\n"
        "            radius: this._configuracoes.get_int('raio') * global.display.get_monitor_scale(0),\n",
    )
    commit(extensao, "Compatibilidade com GNOME 50 (tarefa #1)",
           {"metadata.json": meta50, "extension.js": ext50}, dia(4, 19, 5))

    # ---------- rascunhos (sem git) ----------
    escrever(proj["rascunhos"], RASCUNHOS)

    # ---------- cópias das tarefas (git worktree) ----------
    def copia(projeto, numero, titulo):
        destino = os.path.join(ws_vitral, "worktrees", projeto, f"t{numero}")
        os.makedirs(os.path.dirname(destino), exist_ok=True)
        branch = f"vitral/t{numero}-{slug(titulo)}"
        base = git(proj[projeto], "rev-parse", "HEAD")
        git(proj[projeto], "worktree", "add", "-q", "-b", branch, destino, "HEAD")
        local = os.path.join(proj[projeto], ".env")
        if os.path.exists(local):
            shutil.copy(local, os.path.join(destino, ".env"))
        return destino, branch, base

    chats, itens, tarefas = [], {}, []

    def guardar(par):
        c, lista = par
        chats.append(c)
        itens[c["id"]] = lista
        return c

    # Ids de antemão (as conversas citam umas às outras).
    C = {k: novo_id() for k in ["carrinho", "contato", "prefs", "ideias", "lentas", "fonte", "gnome50",
                                "csv", "seo", "deploy", "rotina"]}
    W = {n: novo_id() for n in range(1, 9)}

    preparacao_loja = verificacao(
        "python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt", True,
        "Successfully installed Django-5.2.7 Pillow-11.3.0 asgiref-3.9.1 gunicorn-23.0.0 "
        "packaging-25.0 sqlparse-0.5.3 whitenoise-6.9.0", 11,
    )

    # ================= #1 extensao: Compatibilidade com GNOME 50 (aprovada, há 4 dias)
    t1 = tarefa(
        1, "extensao", C["gnome50"], W[1], "Compatibilidade com GNOME 50",
        "Fazer a extensão carregar no GNOME Shell 50 sem avisos, mantendo o 48 e o 49.",
        ["metadata.json lista o 50", "O desfoque respeita a escala do monitor no 50",
         "Nada muda no 48 e no 49"],
        "gnome-extensions pack --force --extra-source=schemas", "Aprovada", dia(4, 18, 20),
        worktree=None, branch="vitral/t1-" + slug("Compatibilidade com GNOME 50"),
        base=git(extensao, "rev-parse", "HEAD~1"),
        relatorio="Acrescentei o 50 em `shell-version` e multipliquei o raio pela escala do monitor "
                  "(no 50 o `Shell.BlurEffect` mede em pixels físicos).",
        ultima_verificacao=verificacao("gnome-extensions pack --force --extra-source=schemas", True,
                                       "", 1),
        revisao={"veredito": "aprovar", "resumo": "Mudança pequena e correta.", "problemas": []},
    )
    tarefas.append(t1)

    # ================= #2 loja: Índices no admin (aprovada, ontem)
    t2 = tarefa(
        2, "loja", C["lentas"], W[2], "Índices e select_related no admin",
        "A página de pedidos do admin faz uma consulta por linha; deixar abaixo de 10 consultas.",
        ["PedidoAdmin com list_select_related", "Índice em Pedido.criado_em e (status, criado_em)",
         "Os testes continuam passando"],
        "python3 manage.py test produtos", "Aprovada", dia(1, 14, 30),
        branch="vitral/t2-" + slug("Índices e select_related no admin"),
        base=git(loja, "rev-parse", "HEAD~2"),
        relatorio="`list_select_related = (\"cliente\",)` no PedidoAdmin e os dois índices em Pedido. "
                  "A lista de 100 pedidos caiu de 214 para 6 consultas.",
        ultima_verificacao=verificacao("python3 manage.py test produtos", True,
                                       "Ran 1 test in 0.214s\n\nOK", 6),
        revisao={"veredito": "aprovar", "resumo": "Índices certos e a consulta caiu como prometido.",
                 "problemas": [{"arquivo": "produtos/models.py", "linha": 22, "gravidade": "baixa",
                                "descricao": "Falta a migração gerada no commit (makemigrations)."}]},
        preparacao=preparacao_loja,
    )
    tarefas.append(t2)

    # ================= #3 loja: Modelo Cupom (aprovada, hoje)
    t3 = tarefa(
        3, "loja", C["carrinho"], W[3], "Modelo Cupom e validação",
        "Criar o modelo Cupom (código único, percentual ou valor fixo, validade e limite de usos).",
        ["Código guardado em maiúsculas e único sem diferenciar caixa",
         "Tipos percentual e fixo", "Testes do modelo"],
        "python3 manage.py test produtos", "Aprovada", hoje(70),
        branch="vitral/t3-" + slug("Modelo Cupom e validação"), base=sha_t2,
        relatorio="Modelo `Cupom` em `produtos/models.py` com `save()` que normaliza o código e dois "
                  "testes novos (maiúsculas e unicidade).",
        ultima_verificacao=verificacao("python3 manage.py test produtos", True,
                                       "Ran 3 tests in 0.402s\n\nOK", 14),
        revisao={"veredito": "aprovar", "resumo": "Modelo simples e bem testado.", "problemas": []},
        preparacao=preparacao_loja,
        ressalva="a verificação falhou num teste instável de data (passa de novo ao repetir)",
    )
    tarefas.append(t3)

    # ================= #4 loja: Aplicar cupom no carrinho (em revisão, diff de verdade)
    titulo4 = "Aplicar cupom no carrinho"
    pasta4, branch4, base4 = copia("loja", 4, titulo4)
    ponto4 = foto(pasta4, "t4")
    views_antes = open(os.path.join(pasta4, "produtos/views.py"), encoding="utf-8").read()
    corpo_antigo = views_antes[views_antes.index("\n\ndef carrinho(request):"):]
    views_depois = views_antes.replace(corpo_antigo, VIEW_CARRINHO_NOVA).replace(
        "from .models import Produto\n",
        "from .cupons import CupomInvalido, aplicar_cupom\nfrom .models import Produto\n",
    )
    testes_antes = open(os.path.join(pasta4, "produtos/tests.py"), encoding="utf-8").read()
    testes_depois = testes_antes.replace(
        "from decimal import Decimal\n", "from datetime import date\nfrom decimal import Decimal\n"
    ) + TESTES_CARRINHO
    escrever(pasta4, {
        "produtos/cupons.py": CUPONS_PY,
        "produtos/views.py": views_depois,
        "produtos/templates/produtos/carrinho.html": CARRINHO_HTML_NOVO,
        "produtos/tests.py": testes_depois,
    })
    stat4 = diff_stat(pasta4, base4)
    linha_teste4 = linha_de(testes_depois, '{"cupom": "VERAO10"}')
    saida4 = (
        "Creating test database for alias 'default'...\n"
        "System check identified no issues (0 silenced).\n"
        "......E\n"
        "======================================================================\n"
        "ERROR: test_cupom_expirado_nao_vale (produtos.tests.CarrinhoTests.test_cupom_expirado_nao_vale)\n"
        "----------------------------------------------------------------------\n"
        "Traceback (most recent call last):\n"
        f"  File \"{pasta4}/produtos/tests.py\", line {linha_teste4}, in test_cupom_expirado_nao_vale\n"
        "    resposta = self.client.post(\"/carrinho/\", {\"cupom\": \"VERAO10\"})\n"
        f"  File \"{pasta4}/produtos/views.py\", line {linha_de(views_depois, '_, desconto = aplicar_cupom')}, in carrinho\n"
        "    _, desconto = aplicar_cupom(codigo, subtotal)\n"
        f"  File \"{pasta4}/produtos/cupons.py\", line {linha_de(CUPONS_PY, 'datetime.now()')}, in aplicar_cupom\n"
        "    if cupom.validade and cupom.validade < datetime.now():\n"
        "                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^\n"
        "TypeError: can't compare datetime.datetime to datetime.date\n\n"
        "----------------------------------------------------------------------\n"
        "Ran 7 tests in 0.618s\n\n"
        "FAILED (errors=1)\n"
        "Destroying test database for alias 'default'..."
    )
    revisao4 = {
        "veredito": "ajustar",
        "resumo": "O desconto funciona, mas cupom vencido derruba a página e o percentual não arredonda.",
        "problemas": [
            {"arquivo": "produtos/cupons.py", "linha": linha_de(CUPONS_PY, "datetime.now()"),
             "gravidade": "alta",
             "descricao": "Compara `validade` (DateField, um `date`) com `datetime.now()`: com um cupom "
                          "vencido a view levanta TypeError e o carrinho dá erro 500. Use "
                          "`timezone.localdate()`."},
            {"arquivo": "produtos/cupons.py", "linha": linha_de(CUPONS_PY, "subtotal * cupom.valor / 100"),
             "gravidade": "media",
             "descricao": "O desconto percentual não é arredondado: 10% de R$ 19,90 dá 1,990 e o total "
                          "sai com três casas. Use `.quantize(Decimal(\"0.01\"))`."},
            {"arquivo": "produtos/views.py", "linha": linha_de(views_depois, 'request.session["cupom"] ='),
             "gravidade": "media",
             "descricao": "O código fica na sessão depois que o pedido fecha: o próximo carrinho herda "
                          "o desconto sem o cliente digitar nada."},
            {"arquivo": "produtos/templates/produtos/carrinho.html",
             "linha": linha_de(CARRINHO_HTML_NOVO, 'class="erro"'), "gravidade": "baixa",
             "descricao": "A mensagem de erro do cupom não tem `role=\"alert\"`; o leitor de tela não "
                          "anuncia."},
        ],
    }
    relatorio4 = (
        "Pronto. O carrinho agora aceita cupom:\n\n"
        "- `produtos/cupons.py` (novo): `aplicar_cupom()` valida o código, a validade e o limite de "
        "usos e devolve o desconto.\n"
        "- `produtos/views.py`: a view `carrinho` lê o campo `cupom`, guarda o código na sessão e "
        "manda `desconto` e `total` para o template.\n"
        "- `carrinho.html`: campo para o código, a linha do desconto e a mensagem de erro.\n"
        "- `tests.py`: 5 testes novos (percentual, fixo, inexistente, limite de usos e vencido).\n\n"
        "`python3 manage.py test produtos.tests.CarrinhoTests -k percentual` passou."
    )
    t4 = tarefa(
        4, "loja", C["carrinho"], W[4], titulo4,
        "No carrinho, o cliente digita o código do cupom; o desconto aparece no total e erros "
        "(inexistente, vencido, esgotado) aparecem embaixo do campo.",
        ["Campo de cupom no carrinho com mensagem de erro", "Desconto percentual e fixo no total",
         "Cupom vencido ou esgotado não vale", "Testes da view do carrinho"],
        "python3 manage.py test produtos", "EmRevisao", hoje(33),
        worktree=pasta4, branch=branch4, base=base4, relatorio=relatorio4,
        ultima_verificacao=verificacao("python3 manage.py test produtos", False, saida4, 19),
        revisao=revisao4, preparacao=preparacao_loja,
    )
    tarefas.append(t4)

    # ================= #5 loja: Histórico de cupons (rodando quando o Vitral fechou -> parada)
    titulo5 = "Histórico de cupons no admin"
    pasta5, branch5, base5 = copia("loja", 5, titulo5)
    ponto5 = foto(pasta5, "t5")
    admin5 = open(os.path.join(pasta5, "produtos/admin.py"), encoding="utf-8").read().replace(
        "from .models import ItemPedido, Pedido, Produto\n",
        "from .models import Cupom, ItemPedido, Pedido, Produto\n",
    ) + """

@admin.register(Cupom)
class CupomAdmin(admin.ModelAdmin):
    list_display = ("codigo", "tipo", "valor", "validade", "usos", "limite_usos")
    search_fields = ("codigo",)
    list_filter = ("tipo",)
"""
    escrever(pasta5, {"produtos/admin.py": admin5})
    t5 = tarefa(
        5, "loja", C["carrinho"], W[5], titulo5,
        "Mostrar no admin os cupons com quantas vezes cada um foi usado e em quais pedidos.",
        ["CupomAdmin com usos e limite", "Pedidos que usaram o cupom na página dele"],
        "python3 manage.py test produtos", "Rodando", hoje(31),
        worktree=pasta5, branch=branch5, base=base5, parada="o Vitral foi fechado no meio",
        preparacao=preparacao_loja,
    )
    tarefas.append(t5)

    # ================= #6 loja: Cupom por categoria (cancelada, cópia guardada)
    titulo6 = "Cupom por categoria"
    pasta6, branch6, base6 = copia("loja", 6, titulo6)
    ponto6 = foto(pasta6, "t6")
    modelos6 = open(os.path.join(pasta6, "produtos/models.py"), encoding="utf-8").read().replace(
        "    usos = models.PositiveIntegerField(default=0)\n",
        "    usos = models.PositiveIntegerField(default=0)\n"
        "    categoria = models.CharField(max_length=60, blank=True, help_text=\"Vazio vale para tudo.\")\n",
    )
    escrever(pasta6, {"produtos/models.py": modelos6})
    t6 = tarefa(
        6, "loja", C["carrinho"], W[6], titulo6,
        "Permitir limitar um cupom a uma categoria de produto.",
        ["Campo categoria no Cupom", "Desconto só sobre os itens da categoria"],
        "python3 manage.py test produtos", "Cancelada", hoje(31),
        worktree=pasta6, branch=branch6, base=base6, preparacao=preparacao_loja,
    )
    tarefas.append(t6)

    # ================= #7 site: Formulário de contato (em revisão, verificação passou)
    titulo7 = "Formulário de contato acessível"
    pasta7, branch7, base7 = copia("site", 7, titulo7)
    ponto7 = foto(pasta7, "t7")
    index7 = open(os.path.join(pasta7, "index.html"), encoding="utf-8").read().replace(
        '      <a href="sobre.html">Sobre</a>\n',
        '      <a href="sobre.html">Sobre</a>\n      <a href="contato.html">Contato</a>\n',
    )
    css7 = open(os.path.join(pasta7, "css/estilo.css"), encoding="utf-8").read() + CSS_FORMULARIO
    escrever(pasta7, {"contato.html": CONTATO_HTML, "index.html": index7, "css/estilo.css": css7})
    verif7 = verificacao("npx --yes html-validate contato.html index.html", True,
                         "✔ contato.html\n✔ index.html\n\n2 arquivos, nenhum problema.", 4)
    revisao7 = {
        "veredito": "aprovar",
        "resumo": "Formulário com rótulos, autocomplete e dica ligada ao botão; só um detalhe de contraste.",
        "problemas": [
            {"arquivo": "css/estilo.css", "linha": linha_de(css7, "::placeholder"), "gravidade": "baixa",
             "descricao": "O placeholder #5c6680 fica abaixo de 4,5:1 sobre #161d30."},
        ],
    }
    relatorio7 = (
        "Criei `contato.html` com nome, e-mail e mensagem (rótulos, `autocomplete` e `required`), "
        "o link no menu do `index.html` e os estilos do formulário no fim de `css/estilo.css`."
    )
    t7 = tarefa(
        7, "site", C["contato"], W[7], titulo7,
        "Página de contato com formulário (nome, e-mail, mensagem) enviado pelo FormSubmit.",
        ["Rótulos ligados aos campos", "Funciona sem JavaScript", "Link no menu", "HTML válido"],
        "npx --yes html-validate contato.html index.html", "EmRevisao", hoje(62),
        worktree=pasta7, branch=branch7, base=base7, relatorio=relatorio7,
        ultima_verificacao=verif7, revisao=revisao7,
    )
    tarefas.append(t7)

    # ================= #8 site: Validação em JS (ajustando, você parou)
    titulo8 = "Validação do formulário em JS"
    pasta8, branch8, base8 = copia("site", 8, titulo8)
    ponto8a = foto(pasta8, "t8")
    validacao_r1 = VALIDACAO_JS.replace("/^[^@\\s]+@[^@\\s]+$/", "/@/")
    assert validacao_r1 != VALIDACAO_JS
    escrever(pasta8, {"js/validacao.js": validacao_r1, "js/validacao.test.js": VALIDACAO_TESTE})
    ponto8b = foto(pasta8, "t8")
    stat8_r1 = diff_stat(pasta8, base8)
    escrever(pasta8, {"js/validacao.js": VALIDACAO_JS})
    revisao8 = {
        "veredito": "ajustar",
        "resumo": "A regra do e-mail aceita qualquer texto com @ e o teste do domínio falha.",
        "problemas": [
            {"arquivo": "js/validacao.js", "linha": linha_de(validacao_r1, "/@/"), "gravidade": "alta",
             "descricao": "`/@/` aceita `ana@` como e-mail válido (é o teste que falha)."},
            {"arquivo": "js/validacao.test.js", "linha": None, "gravidade": "media",
             "descricao": "Falta um teste para mensagem curta (menos de 10 letras)."},
        ],
    }
    t8 = tarefa(
        8, "site", C["contato"], W[8], titulo8,
        "Validar o formulário de contato no navegador antes de enviar, sem bloquear quem está sem JS.",
        ["Mensagens perto de cada campo", "aria-invalid nos campos com erro", "Teste com node --test"],
        "node --test js/", "Ajustando", hoje(60), rodada=2,
        worktree=pasta8, branch=branch8, base=base8, parada="você interrompeu",
        relatorio="Validação em `js/validacao.js` com mensagens por campo, `aria-invalid` e foco no "
                  "primeiro erro; testes em `js/validacao.test.js`.",
        ultima_verificacao=verificacao(
            "node --test js/", False,
            "✔ aceita formulário completo (0.6ms)\n✖ recusa e-mail sem domínio (0.9ms)\n"
            "  AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:\n\n  0 !== 1\n\n"
            "ℹ tests 2\nℹ pass 1\nℹ fail 1", 2),
        revisao=revisao8,
    )
    tarefas.append(t8)

    # ================================================= conversas
    raiz_loja, raiz_site, raiz_ext, raiz_rasc = proj["loja"], proj["site"], proj["extensao"], proj["rascunhos"]

    # ---------- C1 loja: Carrinho com cupom (a mais recente, abre ao iniciar) ----------
    c1_itens = [
        usuario(
            "Quero cupom de desconto no carrinho da loja: o cliente digita o código, o desconto "
            "aparece no total e o pedido guarda qual cupom foi usado. Pode ser percentual ou valor "
            "fixo, com validade e limite de usos."
        ),
        ferramenta("Read", "README.md", LOJA_INICIAL["README.md"][:400]),
        ferramenta("Read", "models.py", LOJA_INICIAL["produtos/models.py"][:600]),
        ferramenta("Read", "views.py", LOJA_CARRINHO["produtos/views.py"][-500:]),
        ferramenta("Grep", "request.session", "produtos/views.py:15:    carrinho = request.session.setdefault(\"carrinho\", {})\n"
                   "produtos/views.py:24:    for id, quantidade in request.session.get(\"carrinho\", {}).items():"),
        ferramenta("Bash", "Rodar os testes atuais", "Ran 1 test in 0.188s\n\nOK"),
        assistente(
            "Li o projeto. Hoje o carrinho vive na sessão (`request.session[\"carrinho\"]`) e o total "
            "é calculado em `produtos/views.py`, na view `carrinho`. Ainda não há nada de desconto.\n\n"
            "**Plano**\n\n"
            "1. **Modelo `Cupom`**: código único, tipo (`percentual` ou `fixo`), valor, validade e "
            "limite de usos.\n"
            "2. **Aplicar no carrinho**: campo para o código, validação e o desconto no total.\n"
            "3. **Histórico de cupons** no admin, com quantas vezes cada um foi usado.\n"
            "4. **Cupom por categoria** (opcional): limitar o cupom a uma categoria de produto.\n\n"
            "A regra do desconto fica num lugar só:\n\n"
            "```python\n"
            "def desconto(cupom, subtotal: Decimal) -> Decimal:\n"
            "    if cupom.tipo == Cupom.PERCENTUAL:\n"
            "        return (subtotal * cupom.valor / 100).quantize(Decimal(\"0.01\"))\n"
            "    return min(cupom.valor, subtotal)\n"
            "```\n\n"
            "Começo pelo modelo, porque as outras tarefas dependem dele, e distribuo o resto quando "
            "ele estiver juntado."
        ),
        criou(t3, W[3], " Copiei para a cópia: .env. Preparação `python3 -m venv .venv && "
              ".venv/bin/pip install -q -r requirements.txt` ok (11s)."),
        assistente("Tarefa #3 criada. Quando o relatório chegar, eu reviso e distribuo o resto."),
        entrega_do_vitral(
            t3, t3["relatorio"],
            " produtos/models.py | 22 ++++++++++++++++++++++\n produtos/tests.py  | 13 ++++++++++++-\n"
            " 2 files changed, 34 insertions(+), 1 deletion(-)",
            t3["ultima_verificacao"], t3["revisao"],
        ),
        vitral("ver_diff", "tarefa #3",
               "diff --git a/produtos/models.py b/produtos/models.py\n+class Cupom(models.Model):\n"
               "+    PERCENTUAL, FIXO = \"percentual\", \"fixo\"\n(…)"),
        vitral("aprovar_tarefa", "tarefa #3",
               f"Tarefa #3 aprovada e juntada em {raiz_loja}:\n{sha_t3[:7]} Modelo Cupom e validação (tarefa #3)"),
        criou(t4, W[4], " Copiei para a cópia: .env. Preparação `python3 -m venv .venv && "
              ".venv/bin/pip install -q -r requirements.txt` ok (11s)."),
        criou(t5, W[5], " Copiei para a cópia: .env."),
        criou(t6, W[6], " Copiei para a cópia: .env."),
        assistente(
            "Juntei o modelo e distribuí as outras três tarefas em paralelo, cada uma na sua cópia. "
            "A #4 e a #5 mexem em `produtos/views.py` e `admin.py`; junto uma de cada vez."
        ),
        usuario("Deixa o cupom por categoria pra depois, pode cancelar essa."),
        vitral("cancelar_tarefa", "tarefa #6",
               "Tarefa #6 cancelada. A cópia dela fica guardada até o usuário limpar (painel Equipe)."),
        assistente("Cancelei a #6. A cópia fica guardada caso você queira retomar a ideia; dá para "
                   "apagar em **Limpar cópia**, no painel da Equipe."),
        entrega_do_vitral(t4, relatorio4, stat4, t4["ultima_verificacao"], revisao4),
        vitral("ver_diff", "tarefa #4",
               "diff --git a/produtos/cupons.py b/produtos/cupons.py\nnew file mode 100644\n(…)"),
        assistente(
            "A #4 chegou, mas **ainda não dá para aprovar**:\n\n"
            "- A verificação falhou em `test_cupom_expirado_nao_vale`: com cupom vencido o carrinho "
            "dá erro 500.\n"
            "- O revisor apontou a causa em `produtos/cupons.py:18` (compara `date` com `datetime`) e "
            "mais três detalhes: arredondamento, cupom que sobra na sessão e o `role=\"alert\"`.\n\n"
            "O conserto principal é uma linha:\n\n"
            "```python\n"
            "if cupom.validade and cupom.validade < timezone.localdate():\n"
            "    raise CupomInvalido(\"Este cupom venceu.\")\n"
            "```\n\n"
            "Quer que eu peça o ajuste ao trabalhador com os quatro pontos do revisor, ou prefere "
            "olhar o diff no **Revisar** antes?"
        ),
    ]
    # Começou ontem à tarde: a conversa longa atravessa a meia-noite e mostra os separadores de data.
    guardar(chat(C["carrinho"], "loja", "Carrinho com cupom de desconto", "Coordenador", raiz_loja,
                 dia(1, 16, 20), hoje(12), c1_itens, custo_usd=1.84, contexto=61420,
                 permitidas=["edicoes", "bash:python3 manage.py test produtos"]))

    # Trabalhador #3 (aprovada: cópia apagada)
    guardar(chat(W[3], "loja", "#3 Modelo Cupom e validação", "Trabalhador",
                 os.path.join(ws_vitral, "worktrees", "loja", "t3"), hoje(70), hoje(45), [
                     entrega("coordenador", pedido_da_tarefa(t3)),
                     ferramenta("Read", "models.py", LOJA_INICIAL["produtos/models.py"][:300]),
                     ferramenta("Edit", "models.py", "The file produtos/models.py has been updated."),
                     ferramenta("Edit", "tests.py", "The file produtos/tests.py has been updated."),
                     ferramenta("Bash", "Rodar os testes", "Ran 3 tests in 0.402s\n\nOK"),
                     assistente(t3["relatorio"]),
                 ], pai=C["carrinho"], tarefa=3, custo_usd=0.31, contexto=22100))

    # Trabalhador #4 (em revisão)
    guardar(chat(W[4], "loja", "#4 " + titulo4, "Trabalhador", pasta4, hoje(33), hoje(20), [
        entrega("coordenador", pedido_da_tarefa(t4), ponto=ponto4),
        ferramenta("Read", "views.py", views_antes[-600:]),
        ferramenta("Read", "models.py", modelos_com_cupom[-500:]),
        ferramenta("Grep", "request.session", "produtos/views.py:15\nprodutos/views.py:24"),
        ferramenta("Write", "cupons.py", "File created successfully at: " + os.path.join(pasta4, "produtos/cupons.py")),
        ferramenta("Edit", "views.py", "The file produtos/views.py has been updated."),
        ferramenta("Edit", "carrinho.html", "The file produtos/templates/produtos/carrinho.html has been updated."),
        ferramenta("Edit", "tests.py", "The file produtos/tests.py has been updated."),
        ferramenta("Bash", "Rodar os testes do carrinho",
                   "Ran 1 test in 0.211s\n\nOK"),
        assistente(relatorio4),
    ], pai=C["carrinho"], tarefa=4, custo_usd=0.52, contexto=48900, lido_em=hoje(20)))

    # Trabalhador #5 (parada: o Vitral fechou no meio)
    guardar(chat(W[5], "loja", "#5 " + titulo5, "Trabalhador", pasta5, hoje(31), hoje(26), [
        entrega("coordenador", pedido_da_tarefa(t5), ponto=ponto5),
        ferramenta("Read", "admin.py", admin_csv[:500]),
        ferramenta("Edit", "admin.py", "The file produtos/admin.py has been updated."),
        ferramenta("Grep", "cupom", "produtos/models.py:42:class Cupom(models.Model):"),
        aviso("O Vitral foi fechado no meio desta tarefa. Use Retomar (ou mande uma mensagem) para "
              "continuar; o relatório vai para o coordenador quando terminar."),
    ], pai=C["carrinho"], tarefa=5, custo_usd=0.12, contexto=18300))

    # Trabalhador #6 (cancelada, cópia guardada)
    guardar(chat(W[6], "loja", "#6 " + titulo6, "Trabalhador", pasta6, hoje(31), hoje(24), [
        entrega("coordenador", pedido_da_tarefa(t6), ponto=ponto6),
        ferramenta("Read", "models.py", modelos_com_cupom[-400:]),
        ferramenta("Edit", "models.py", "The file produtos/models.py has been updated."),
        aviso("Tarefa cancelada pelo coordenador: o usuário pediu para deixar o cupom por categoria para depois"),
    ], pai=C["carrinho"], tarefa=6, custo_usd=0.06, contexto=9800))

    # ---------- C2 site: Página de contato (não lida) ----------
    c2_itens = [
        usuario("Preciso de uma página de contato no site, com formulário simples (nome, e-mail e "
                "mensagem). Nada de backend: pode usar o FormSubmit."),
        ferramenta("Glob", "**/*.html", "index.html\nsobre.html"),
        ferramenta("Read", "index.html", SITE_INICIAL["index.html"][:500]),
        ferramenta("Read", "estilo.css", css_fonte[:400]),
        assistente("Vou separar em duas tarefas: a página em HTML puro (funciona sem JavaScript) e a "
                   "validação no navegador por cima dela."),
        criou(t7, W[7]),
        criou(t8, W[8]),
        entrega_do_vitral(t7, relatorio7, diff_stat(pasta7, base7), verif7, revisao7),
        entrega_do_vitral(t8, t8["relatorio"], stat8_r1, t8["ultima_verificacao"], revisao8),
        vitral("pedir_ajuste", "tarefa #8", "Ajuste enviado (rodada 2) com os problemas do revisor. O novo "
               "relatório chega sozinho."),
        assistente(
            "A página de contato (#7) está pronta para você revisar: HTML válido e o revisor aprovaria "
            "(só um detalhe de contraste no placeholder).\n\n"
            "Na validação (#8) pedi um ajuste: a regra do e-mail aceitava `@` sozinho e faltavam testes. "
            "Você parou essa rodada no meio; quando quiser, é só **Retomar** no cartão da tarefa."
        ),
    ]
    guardar(chat(C["contato"], "site", "Página de contato com formulário", "Coordenador", raiz_site,
                 hoje(65), hoje(48), c2_itens, custo_usd=0.97, contexto=38800,
                 permissao="plan", modo_antes_do_plano="acceptEdits", lido_em=hoje(58)))
    guardar(chat(W[7], "site", "#7 " + titulo7, "Trabalhador", pasta7, hoje(62), hoje(55), [
        entrega("coordenador", pedido_da_tarefa(t7), ponto=ponto7),
        ferramenta("Read", "index.html", SITE_INICIAL["index.html"][:300]),
        ferramenta("Write", "contato.html", "File created successfully at: " + os.path.join(pasta7, "contato.html")),
        ferramenta("Edit", "index.html", "The file index.html has been updated."),
        ferramenta("Edit", "estilo.css", "The file css/estilo.css has been updated."),
        ferramenta("Bash", "Validar o HTML", "✔ contato.html\n✔ index.html"),
        assistente(relatorio7),
    ], pai=C["contato"], tarefa=7, custo_usd=0.18, contexto=15200))
    guardar(chat(W[8], "site", "#8 " + titulo8, "Trabalhador", pasta8, hoje(60), hoje(49), [
        entrega("coordenador", pedido_da_tarefa(t8), ponto=ponto8a),
        ferramenta("Write", "validacao.js", "File created successfully at: " + os.path.join(pasta8, "js/validacao.js")),
        ferramenta("Write", "validacao.test.js", "File created successfully at: " + os.path.join(pasta8, "js/validacao.test.js")),
        ferramenta("Bash", "Rodar os testes", t8["ultima_verificacao"]["saida"], erro=True),
        assistente(t8["relatorio"]),
        entrega("coordenador",
                "Ajuste pedido pelo coordenador (rodada 2):\nTroque a regra do e-mail por uma que exija "
                "texto dos dois lados do @ e escreva os testes com node --test.\n\nProblemas do revisor:\n"
                + "".join(f"{n}. [{p['gravidade']}] {p['arquivo']}{':' + str(p['linha']) if p['linha'] else ''}: "
                          f"{p['descricao']}\n" for n, p in enumerate(revisao8["problemas"], 1))
                + "\nQuando terminar, mande um novo relatório.", ponto=ponto8b),
        ferramenta("Read", "validacao.test.js", VALIDACAO_TESTE[:300]),
        ferramenta("Edit", "validacao.js", "The file js/validacao.js has been updated."),
        ferramenta("Bash", "Rodar os testes", None),
        aviso("Interrompido."),
    ], pai=C["contato"], tarefa=8, custo_usd=0.21, contexto=17600))

    # ---------- C3 extensao: turno cortado (Retomar) ----------
    guardar(chat(C["prefs"], "extensao", "Lista de apps nas preferências", "Coordenador", raiz_ext,
                 hoje(140), hoje(128), [
                     usuario("Nas preferências da extensão, quero escolher em quais apps o desfoque vale "
                             "(hoje só dá pelo dconf)."),
                     ferramenta("Read", "prefs.js", PREFS_JS[:500]),
                     ferramenta("Read", "gschema.xml", EXTENSAO_INICIAL[
                         "schemas/org.gnome.shell.extensions.desfoque.gschema.xml"][:400]),
                     ferramenta("Grep", "get_strv", "extension.js:21:        const apps = this._configuracoes.get_strv('apps');"),
                     assistente("A chave `apps` já existe no schema; falta a interface. Vou olhar como "
                                "o GNOME 50 lista os apps instalados para montar a escolha."),
                     ferramenta("Bash", "Listar os .desktop instalados", None),
                     aviso("O Vitral foi fechado no meio deste turno. Use Retomar para o Claude continuar "
                           "de onde parou."),
                 ], turno_aberto=True, custo_usd=0.22, contexto=14100))

    # ---------- C4 rascunhos (sem git): conversa com erro ----------
    guardar(chat(C["ideias"], "rascunhos", "Roteiro do episódio 12", "Coordenador", raiz_rasc,
                 hoje(300), hoje(290), [
                     usuario("Junta as ideias do ideias-videos.md com o roteiro do episódio 12 e me "
                             "sugere uma ordem melhor para as cenas."),
                     ferramenta("Read", "ideias-videos.md", RASCUNHOS["ideias-videos.md"]),
                     ferramenta("Read", "roteiro-ep12.md", RASCUNHOS["roteiro-ep12.md"]),
                     aviso("API Error: 529 overloaded; tentando de novo em 60 s."),
                     aviso("Tentando de novo."),
                     aviso("Falhou: API Error: 529 {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\","
                           "\"message\":\"Overloaded\"}}", erro=True),
                 ], custo_usd=0.04, contexto=6200))

    # ---------- C11 loja: aberta por uma rotina hoje cedo ----------
    guardar(chat(C["rotina"], "loja", "Rotina: Conferir dependências desatualizadas", "Coordenador", raiz_loja,
                 hoje(420), hoje(412), [
                     usuario("Confira se há dependências desatualizadas ou com alerta de segurança e me "
                             "diga o que vale atualizar."),
                     ferramenta("Read", "requirements.txt", LOJA_INICIAL["requirements.txt"]),
                     ferramenta("Bash", "Listar pacotes desatualizados",
                                "Package    Version Latest Type\n---------- ------- ------ -----\n"
                                "Django     5.2.7   5.2.8  wheel\nPillow     11.3.0  12.0.0 wheel"),
                     ferramenta("Bash", "Procurar alertas de segurança (pip-audit)", "No known vulnerabilities found"),
                     assistente(
                         "Nada urgente: nenhum alerta de segurança.\n\n"
                         "- **Django 5.2.7 → 5.2.8**: correção de versão, vale atualizar já.\n"
                         "- **Pillow 11.3.0 → 12.0.0**: versão nova grande; esperaria o próximo ciclo.\n\n"
                         "Se quiser, crio uma tarefa para subir o Django e rodar os testes."
                     ),
                 ], custo_usd=0.09, contexto=9900))

    # ---------- C5 loja (fixada, ontem): resumo do coordenador ----------
    guardar(chat(C["lentas"], "loja", "Consultas lentas no admin de pedidos", "Coordenador", raiz_loja,
                 dia(1, 14, 10), dia(1, 15, 20), [
                     usuario("A página de pedidos do admin está demorando uns 4 segundos. Descobre o porquê?"),
                     ferramenta("Read", "admin.py", admin_csv[:500]),
                     ferramenta("Bash", "Contar as consultas da lista de pedidos",
                                "214 consultas em 3.92s (django.db.backends)"),
                     assistente("São 214 consultas para 100 pedidos: uma para o cliente e uma para o "
                                "total de cada linha. Vou pedir `select_related` e índices."),
                     criou(t2, W[2]),
                     entrega_do_vitral(t2, t2["relatorio"],
                                       " produtos/admin.py  | 1 +\n produtos/models.py | 5 ++++-\n"
                                       " 2 files changed, 5 insertions(+), 1 deletion(-)",
                                       t2["ultima_verificacao"], t2["revisao"]),
                     vitral("aprovar_tarefa", "tarefa #2",
                            f"Tarefa #2 aprovada e juntada em {raiz_loja}:\n{sha_t2[:7]} Índices e "
                            "select_related no admin (tarefa #2)"),
                     assistente(
                         "Pronto: a lista de pedidos do admin caiu de **214 para 6 consultas** "
                         "(de 3,9 s para 0,2 s).\n\n"
                         "- `PedidoAdmin` usa `list_select_related = (\"cliente\",)`.\n"
                         "- Índices novos em `Pedido.criado_em` e em `(status, criado_em)`.\n\n"
                         "Está juntado no projeto. Falta só gerar a migração "
                         "(`python3 manage.py makemigrations produtos`), que o revisor lembrou."
                     ),
                 ], fixado=True, custo_usd=0.58, contexto=27400))
    guardar(chat(W[2], "loja", "#2 Índices e select_related no admin", "Trabalhador",
                 os.path.join(ws_vitral, "worktrees", "loja", "t2"), dia(1, 14, 30), dia(1, 15, 0), [
                     entrega("coordenador", pedido_da_tarefa(t2)),
                     ferramenta("Edit", "admin.py", "The file produtos/admin.py has been updated."),
                     ferramenta("Edit", "models.py", "The file produtos/models.py has been updated."),
                     ferramenta("Bash", "Contar as consultas de novo", "6 consultas em 0.21s"),
                     assistente(t2["relatorio"]),
                 ], pai=C["lentas"], tarefa=2, custo_usd=0.14, contexto=12800))

    # ---------- C6 site (ontem): resposta com código ----------
    guardar(chat(C["fonte"], "site", "Fonte serifada no cabeçalho", "Coordenador", raiz_site,
                 dia(1, 18, 20), dia(1, 18, 32), [
                     usuario("Que fonte combina com o nome do ateliê no cabeçalho? Quero algo com cara "
                             "de artesanal, mas legível."),
                     assistente(
                         "Para o nome **Ateliê Vitral**, uma serifada com contraste suave funciona bem:\n\n"
                         "| Fonte | Por quê |\n| --- | --- |\n"
                         "| **Fraunces** | serifas macias, bem \"feito à mão\" |\n"
                         "| Playfair Display | elegante, mais contraste |\n"
                         "| Lora | discreta, ótima em tamanhos pequenos |\n\n"
                         "Com a Fraunces só no nome e a Inter no resto:\n\n"
                         "```css\n.marca {\n  font-family: \"Fraunces\", Georgia, serif;\n"
                         "  font-size: 1.5rem;\n}\n```\n\n"
                         "Se gostar, é só colar no fim de `css/estilo.css`."
                     ),
                     usuario("Gostei da Fraunces, já coloquei. Valeu!"),
                     assistente("Ficou ótimo. Se quiser, depois a gente carrega a fonte localmente para "
                                "o site não depender do Google Fonts."),
                 ], custo_usd=0.05, contexto=4100))

    # ---------- C7 extensao (há 4 dias): resumo ----------
    guardar(chat(C["gnome50"], "extensao", "Compatibilidade com GNOME 50", "Coordenador", raiz_ext,
                 dia(4, 18, 0), dia(4, 19, 10), [
                     usuario("O Fedora 44 veio com o GNOME 50 e a extensão parou de carregar. Dá uma olhada?"),
                     ferramenta("Bash", "Ver o log do Shell", "JS ERROR: Extension desfoque-janelas@victor.local: "
                                "incompatible shell version", erro=True),
                     ferramenta("Read", "metadata.json", EXTENSAO_INICIAL["metadata.json"]),
                     criou(t1, W[1]),
                     entrega_do_vitral(t1, t1["relatorio"],
                                       " extension.js  | 3 ++-\n metadata.json | 3 ++-\n 2 files changed, 4 insertions(+), 2 deletions(-)",
                                       t1["ultima_verificacao"], t1["revisao"]),
                     vitral("aprovar_tarefa", "tarefa #1",
                            "Tarefa #1 aprovada e juntada em " + raiz_ext + ":\n" + git(extensao, "rev-parse", "--short", "HEAD")
                            + " Compatibilidade com GNOME 50 (tarefa #1)"),
                     assistente("A extensão volta a carregar no GNOME 50: o `metadata.json` lista o 50 e o "
                                "raio do desfoque passou a levar em conta a escala do monitor. Reinstale "
                                "com `gnome-extensions install --force` e saia da sessão para recarregar."),
                 ], custo_usd=0.33, contexto=19700))
    guardar(chat(W[1], "extensao", "#1 Compatibilidade com GNOME 50", "Trabalhador",
                 os.path.join(ws_vitral, "worktrees", "extensao", "t1"), dia(4, 18, 20), dia(4, 18, 50), [
                     entrega("coordenador", pedido_da_tarefa(t1)),
                     ferramenta("Edit", "metadata.json", "The file metadata.json has been updated."),
                     ferramenta("Edit", "extension.js", "The file extension.js has been updated."),
                     assistente(t1["relatorio"]),
                 ], pai=C["gnome50"], tarefa=1, custo_usd=0.09, contexto=8800))

    # ---------- C8 loja (há 6 dias) ----------
    guardar(chat(C["csv"], "loja", "Exportar pedidos em CSV", "Coordenador", raiz_loja,
                 dia(6, 16, 0), dia(6, 16, 45), [
                     usuario("Como exporto os pedidos selecionados no admin para uma planilha?"),
                     assistente(
                         "Uma *action* no `PedidoAdmin` resolve, sem dependência nova:\n\n"
                         "```python\n@admin.action(description=\"Exportar selecionados em CSV\")\n"
                         "def exportar_csv(self, request, queryset):\n"
                         "    resposta = HttpResponse(content_type=\"text/csv; charset=utf-8\")\n"
                         "    escritor = csv.writer(resposta, delimiter=\";\")\n"
                         "    for p in queryset:\n"
                         "        escritor.writerow([p.id, p.cliente, p.get_status_display(), p.total()])\n"
                         "    return resposta\n```\n\n"
                         "Com `;` como separador o Excel em português abre direto."
                     ),
                 ], custo_usd=0.03, contexto=3500))

    # ---------- C9 site (há 12 dias) ----------
    guardar(chat(C["seo"], "site", "SEO e meta tags", "Coordenador", raiz_site,
                 dia(12, 10, 0), dia(12, 10, 20), [
                     usuario("O que falta de SEO básico no site?"),
                     assistente(
                         "O essencial que falta:\n\n"
                         "1. `<meta name=\"description\">` em cada página.\n"
                         "2. Open Graph (`og:title`, `og:image`) para o link ficar bonito no WhatsApp.\n"
                         "3. `sitemap.xml` e `robots.txt` na raiz.\n"
                         "4. Texto alternativo nas fotos dos vitrais."
                     ),
                 ], custo_usd=0.02, contexto=2900))

    # ---------- C10 loja (arquivada) ----------
    guardar(chat(C["deploy"], "loja", "Deploy no Coolify", "Coordenador", raiz_loja,
                 dia(9, 20, 0), dia(9, 20, 40), [
                     usuario("Como subo a loja no Coolify da VPS?"),
                     assistente("Com o Nixpacks do Coolify: defina `DJANGO_SECRET_KEY`, `DJANGO_HOSTS` e "
                                "o comando `gunicorn loja.wsgi`, e ligue o volume em `/app/media`."),
                 ], arquivado=True, custo_usd=0.02, contexto=2400))

    # ---------- C11 site (vazio): aberto e nunca usado; a migração o traz como chat de projeto ----------
    guardar(chat(novo_id(), "site", "Nova conversa", "Coordenador", raiz_site, dia(1, 9, 0), dia(1, 9, 0), [],
                 sessao_criada=False, modelo_atual=None))

    # ================================================= gravação

    estado = {"chats": chats, "tarefas": tarefas, "proxima_tarefa": 9, "filas": {}}
    gravar(os.path.join(ws_vitral, "estado.json"), estado)
    for id, lista in itens.items():
        gravar(os.path.join(ws_vitral, "chats", f"{id}.json"), lista)

    hoje_txt = data_hoje()
    config = {
        "permissao": "acceptEdits",
        "modelo_coordenador": "opus",
        "modelo_trabalhador": "sonnet",
        "max_trabalhadores": 4,
        "max_rodadas": 3,
        # Nunca 0 dias de diferença: com 30, nada aqui (tudo tem menos de 16 dias) é limpo ao abrir.
        "limpar_copias_dias": 30,
        "conta": "",
        "revisor": True,
        "modelo_revisor": "",
        "mcp_trabalhadores": False,
        "projetos": {
            "loja": {
                "preparacao": "python3 -m venv .venv && .venv/bin/pip install -q -r requirements.txt",
                "rodar": ".venv/bin/python manage.py runserver",
                "copiar": [".env*"],
                "icone": "shopping-bag",
                "cor": "#f49ac1",
            },
            "site": {"preparacao": "", "rodar": "python3 -m http.server 8080", "copiar": [],
                     "icone": "", "cor": ""},
            "extensao": {"preparacao": "", "rodar": "", "copiar": [], "icone": "", "cor": "#5ad1c4"},
        },
        # As ativas já "rodaram hoje": rodar_rotinas só dispara quando `ultima` não é hoje.
        "rotinas": [
            {"id": novo_id(), "projeto": "loja",
             "texto": "Confira se há dependências desatualizadas ou com alerta de segurança e me diga o que vale atualizar.",
             "hora": 8 * 60, "dias": [1, 2, 3, 4, 5], "ativa": True, "ultima": hoje_txt},
            {"id": novo_id(), "projeto": "site",
             "texto": "Procure links quebrados e imagens sem texto alternativo no site.",
             "hora": 18 * 60 + 30, "dias": [1, 3, 5], "ativa": True, "ultima": hoje_txt},
            {"id": novo_id(), "projeto": "extensao",
             "texto": "Veja se saiu versão nova do GNOME Shell e se a extensão precisa de ajuste.",
             "hora": 10 * 60, "dias": [0], "ativa": False, "ultima": ""},
        ],
        "arquivar_apos_dias": 30,
    }
    gravar(os.path.join(ws_vitral, "config.json"), config)
    os.makedirs(os.path.join(ws_vitral, "notas"), exist_ok=True)
    with open(os.path.join(ws_vitral, "notas", "loja.md"), "w", encoding="utf-8") as f:
        f.write("- Preços e descontos sempre em Decimal, nunca float.\n"
                "- Cupom guarda o código em maiúsculas; a busca ignora caixa.\n"
                "- Uma junção por vez quando duas tarefas mexem em produtos/views.py.\n")
    with open(os.path.join(ws_vitral, "notas", "site.md"), "w", encoding="utf-8") as f:
        f.write("- O site funciona sem JavaScript; JS só melhora.\n")
    return chats, tarefas


def gravar(caminho, valor):
    os.makedirs(os.path.dirname(caminho), exist_ok=True)
    with open(caminho, "w", encoding="utf-8") as f:
        json.dump(valor, f, ensure_ascii=False, indent=2)


def main():
    args = [a for a in sys.argv[1:] if a != "--16"]
    if len(args) != 1:
        sys.exit("uso: python3 -I ferramentas/demo_vitral.py <pasta_destino> [--16]")
    destino = os.path.abspath(args[0])
    if os.path.lexists(destino):
        if os.path.isfile(os.path.join(destino, MARCA)):
            shutil.rmtree(destino)
        elif os.path.isdir(destino) and not os.listdir(destino):
            os.rmdir(destino)
        else:
            sys.exit(f"{destino} já existe e não é uma vitrine gerada por este script; não apago.")
    os.makedirs(destino)
    raiz = os.path.realpath(destino)
    with open(os.path.join(raiz, MARCA), "w", encoding="utf-8") as f:
        f.write("Pasta gerada por ferramentas/demo_vitral.py; pode ser apagada e gerada de novo.\n")
    chats, tarefas = gerar(raiz)
    if "--16" in sys.argv[1:]:
        projetos_extras(raiz)
    estados = ", ".join(f"#{t['id']} {t['estado']}{' (parada)' if t['parada'] else ''}" for t in tarefas)
    print(f"Vitrine em {raiz}: {len(chats)} chats, {len(tarefas)} tarefas ({estados}).")


if __name__ == "__main__":
    main()
