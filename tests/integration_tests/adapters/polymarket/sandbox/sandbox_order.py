# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from py_clob_client.client import BalanceAllowanceParams
from py_clob_client.client import OrderArgs
from py_clob_client.client import PartialCreateOrderOptions
from py_clob_client.clob_types import AssetType

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def place_order() -> None:
    client = get_polymarket_http_client()

    params = BalanceAllowanceParams(
        asset_type=AssetType.COLLATERAL,
        signature_type=0,
    )

    print(f"Updating {params}")
    response = client.update_balance_allowance(params)
    print(response)

    response = client.get_balance_allowance(params)
    print(response)

    response = client.cancel_all()
    print(response)

    # "25143473975747606484038304917293813549571262015788668262095587119656373441253"

    order_args = OrderArgs(
        price=0.001,
        token_id="3642309182816755995211647069086230404892359515361325090555875625429003317932",
        size=5.004282,
        # size=5,
        side="SELL",
    )
    options = PartialCreateOrderOptions(neg_risk=False)
    signed_order = client.create_order(order_args, options=options)

    response = client.post_order(signed_order)
    print(response)


if __name__ == "__main__":
    place_order()
