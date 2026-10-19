"""pool起動時の元群を固定し、生存する待機側の終了前に回収する。"""
from contextlib import contextmanager
import os

from jade_pdf2docx_group_retirement import retire_original
from jade_pdf2docx_slot_retirement import pin_slice


class SlotCompletion:
    """同じ受付gate内で作られたjobの親と元群を保持する。"""

    def __init__(self, parent, control_group, manager, unit, inspect):
        """起動前の親FDを受け取り、群の所有権をまだ確定しない。"""
        self.parent = parent
        self.control_group = control_group
        self.manager = manager
        self.unit = unit
        self.service = unit + ".service"
        self.inspect = inspect
        self.original = None
        self.captured = False

    def capture(self, token, *, done):
        """最初のgateを解放する前に元群を固定し、観測不足を成功にしない。"""
        properties = self.inspect(self.manager, self.unit)
        if properties.get("LoadState") == "loaded":
            if (properties.get("Description") != "jade-pdf2docx-job-" + token or
                    properties.get("ControlGroup") not in
                    ("", self.control_group + "/" + self.service)):
                raise ValueError("job identity changed before completion pin")
        elif properties.get("LoadState") != "not-found":
            raise ValueError("cannot inspect original job for completion")
        try:
            self.original = os.open(self.service, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW |
                                    os.O_CLOEXEC, dir_fd=self.parent)
        except FileNotFoundError:
            if not done():
                raise ValueError("original group not observed before releasing admission gate")
            # 同じgate内で起動前に存在しなかった群が、終了後も存在しない。
        self.captured = True

    def retire(self, token):
        """再取得したgate内で元群だけを回収し、再利用済みの群には触れない。"""
        if not self.captured:
            raise ValueError("original job completion was not observed")
        if self.original is not None:
            retire_original(self.original, self.manager, self.service, token, self.inspect)

    def close(self):
        """子へ継承しない元群FDを、成功/例外のいずれでも閉じる。"""
        if self.original is not None:
            os.close(self.original)
            self.original = None


@contextmanager
def watch_slot(manager, slice_name, unit, inspect):
    """起動前の空slotと親を固定し、起動後の観測まで最初のgateで保護する。"""
    properties = inspect(manager, slice_name)
    group = properties.get("ControlGroup")
    if properties.get("LoadState") != "loaded" or not isinstance(group, str):
        raise ValueError("cannot locate pool for completion observation")
    service = unit + ".service"
    if "/" in service or not unit.startswith("jade-pdf2docx-" + slice_name[:-6] + "-slot-"):
        raise ValueError("completion slot outside configured pool")
    with pin_slice(group, slice_name) as parent:
        try:
            os.stat(service, dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise ValueError("completion slot was not retired before launch")
        watcher = SlotCompletion(parent, group, manager, unit, inspect)
        try:
            yield watcher
        finally:
            watcher.close()
